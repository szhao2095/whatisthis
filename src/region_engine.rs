//! Sliding-window region profiler with multi-label aggregation.
//!
//! Breaks a file into overlapping windows, runs a scorer on each, and
//! aggregates per-label peak/coverage/persistence into a file-level verdict.
//!
//! This is the foundation for hostile-mode analysis where a single whole-file
//! verdict is insufficient: malicious payloads often occupy only one region
//! while benign-looking padding or a wrapper dominates the rest.

/// Scores and byte range for one window.
#[derive(Debug, Clone)]
pub struct RegionVerdict {
    /// Byte offset of the window start in the original file.
    pub start: usize,
    /// Byte offset one past the end of this window.
    pub end: usize,
    /// Per-label scores, sorted descending. From whatever scorer was used
    /// (cosine similarity for TF-ICF, log-prob sum for Bayes).
    pub scores: Vec<(&'static str, f64)>,
}

impl RegionVerdict {
    /// Top label for this window (empty string if scores is empty).
    pub fn top_label(&self) -> &'static str {
        self.scores.first().map(|(l, _)| *l).unwrap_or("")
    }
}

/// File-level aggregation result.
#[derive(Debug, Clone)]
pub struct FileVerdict {
    pub regions: Vec<RegionVerdict>,
    /// Multi-label aggregate scores sorted descending.
    /// Labels with aggregate score > FILE_THRESHOLD are included;
    /// at least 1 label is always included (the argmax).
    pub labels: Vec<(&'static str, f64)>,
}

// ── Aggregation constants ────────────────────────────────────────────────────

/// A window's score for a label must exceed this to count toward coverage.
const COVERAGE_THRESHOLD: f64 = 0.30;

/// Aggregate score weights.
const W_PEAK: f64 = 0.50;
const W_COVERAGE: f64 = 0.30;
const W_PERSIST: f64 = 0.20;

/// Labels with aggregate score below this are excluded from the final list.
const FILE_THRESHOLD: f64 = 0.20;

/// Number of top windows used to compute persistence.
const PERSIST_TOP_K: usize = 3;

// ── Public API ───────────────────────────────────────────────────────────────

/// Profile `content` (raw bytes) using overlapping windows of size `window`
/// with step `stride`. For each window, `scorer` is called with the UTF-8
/// string slice of that window and the candidates list.
///
/// `scorer` must return `Vec<(&'static str, f64)>` sorted descending by score.
///
/// Returns both per-region verdicts and the file-level multi-label verdict.
pub fn region_profile(
    content: &[u8],
    window: usize,
    stride: usize,
    scorer: &dyn Fn(&str, &[&'static str]) -> Vec<(&'static str, f64)>,
    candidates: &[&'static str],
) -> FileVerdict {
    let window = window.max(1);
    let stride = stride.max(1);
    let total = content.len();

    if total == 0 {
        return FileVerdict { regions: vec![], labels: vec![] };
    }

    let mut regions: Vec<RegionVerdict> = Vec::new();
    let mut offset = 0usize;

    while offset < total {
        let end = (offset + window).min(total);
        let chunk_bytes = &content[offset..end];

        // Lossy UTF-8 decode so binary-adjacent content doesn't abort.
        let text = String::from_utf8_lossy(chunk_bytes);
        let scores = scorer(&text, candidates);

        regions.push(RegionVerdict { start: offset, end, scores });

        if end >= total {
            break;
        }
        offset += stride;
    }

    let labels = aggregate_labels(&regions, total);
    FileVerdict { regions, labels }
}

// ── Aggregation ──────────────────────────────────────────────────────────────

fn aggregate_labels(regions: &[RegionVerdict], total_bytes: usize) -> Vec<(&'static str, f64)> {
    if regions.is_empty() {
        return vec![];
    }

    // Collect all label names that appear in any region.
    let mut all_labels: Vec<&'static str> = Vec::new();
    for r in regions {
        for (lang, _) in &r.scores {
            if !all_labels.contains(lang) {
                all_labels.push(lang);
            }
        }
    }

    let mut file_scores: Vec<(&'static str, f64)> = all_labels
        .into_iter()
        .map(|label| {
            let s = aggregate_one(label, regions, total_bytes);
            (label, s)
        })
        .collect();

    file_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Always include at least the argmax; add others above FILE_THRESHOLD.
    let mut out: Vec<(&'static str, f64)> = Vec::new();
    for (i, (label, score)) in file_scores.iter().enumerate() {
        if i == 0 || *score > FILE_THRESHOLD {
            out.push((label, *score));
        }
    }
    out
}

fn aggregate_one(label: &'static str, regions: &[RegionVerdict], total_bytes: usize) -> f64 {
    // Peak: highest score in any window.
    let peak = regions
        .iter()
        .map(|r| score_for(label, &r.scores))
        .fold(f64::NEG_INFINITY, f64::max);
    let peak = if peak.is_finite() { peak } else { 0.0 };

    // Coverage: fraction of bytes in windows where score > threshold.
    let covered_bytes: usize = regions
        .iter()
        .filter(|r| score_for(label, &r.scores) > COVERAGE_THRESHOLD)
        .map(|r| r.end - r.start)
        .sum();
    let coverage = if total_bytes > 0 {
        covered_bytes as f64 / total_bytes as f64
    } else {
        0.0
    };

    // Persistence: mean of top-k window scores.
    let mut window_scores: Vec<f64> = regions
        .iter()
        .map(|r| score_for(label, &r.scores))
        .collect();
    window_scores.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let k = PERSIST_TOP_K.min(window_scores.len());
    let persist = if k > 0 {
        window_scores[..k].iter().sum::<f64>() / k as f64
    } else {
        0.0
    };

    W_PEAK * peak + W_COVERAGE * coverage + W_PERSIST * persist
}

fn score_for(label: &'static str, scores: &[(&'static str, f64)]) -> f64 {
    scores
        .iter()
        .find(|(l, _)| *l == label)
        .map(|(_, s)| *s)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_scorer(content: &str, _candidates: &[&'static str]) -> Vec<(&'static str, f64)> {
        // Trivially classify by which keyword appears more.
        let rust_count = content.matches("fn").count() + content.matches("let").count();
        let py_count = content.matches("def").count() + content.matches("import").count();
        if rust_count >= py_count {
            vec![("Rust", rust_count as f64 + 1.0), ("Python", py_count as f64)]
        } else {
            vec![("Python", py_count as f64 + 1.0), ("Rust", rust_count as f64)]
        }
    }

    #[test]
    fn empty_content_returns_empty() {
        let v = region_profile(&[], 1024, 512, &dummy_scorer, &[]);
        assert!(v.regions.is_empty());
        assert!(v.labels.is_empty());
    }

    #[test]
    fn single_window_content() {
        let content = b"fn main() { let x = 1; }";
        let v = region_profile(content, 4096, 2048, &dummy_scorer, &[]);
        assert_eq!(v.regions.len(), 1);
        assert_eq!(v.regions[0].start, 0);
        assert_eq!(v.regions[0].end, content.len());
        assert!(!v.labels.is_empty());
        assert_eq!(v.labels[0].0, "Rust");
    }

    #[test]
    fn multiple_windows_produced() {
        let content = vec![b'x'; 10000];
        let v = region_profile(&content, 1024, 512, &dummy_scorer, &[]);
        // (10000 - 1024) / 512 + 1 = ~18 windows
        assert!(v.regions.len() > 5);
    }

    #[test]
    fn labels_sorted_descending() {
        let content = b"fn foo() { let a = 1; let b = 2; fn bar() {} }";
        let v = region_profile(content, 4096, 4096, &dummy_scorer, &[]);
        for pair in v.labels.windows(2) {
            assert!(pair[0].1 >= pair[1].1, "labels not sorted descending");
        }
    }

    #[test]
    fn region_top_label_matches_scores_first() {
        let content = b"fn main() {}";
        let v = region_profile(content, 4096, 4096, &dummy_scorer, &[]);
        let r = &v.regions[0];
        assert_eq!(r.top_label(), r.scores[0].0);
    }
}
