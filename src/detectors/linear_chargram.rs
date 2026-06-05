use std::collections::HashMap;

include!("../codegen/linear-chargram-model.rs");

/// Maximum n-gram length accepted. Guards against pathological input.
const MAX_NGRAM_BYTES: usize = 10;

/// TF cap — must match codegen's CHARGRAM_TF_CAP.
const TF_CAP: u32 = 100;

/// n-gram sizes to extract at inference time. Must match codegen's CHARGRAM_NS.
const CHARGRAM_NS: &[usize] = &[4];

/// Classify `content` using the char n-gram centroid model. Behaves like
/// `classify` and `classify_tficf`: if `candidates` is empty, scores all
/// languages in the model; otherwise scores only the given candidates.
///
/// Returns the language with the highest cosine similarity.
pub fn classify_linear(content: &str, candidates: &[&'static str]) -> &'static str {
    let scores = classify_linear_scored(content, candidates);
    scores.first().map(|(l, _)| *l).unwrap_or_else(|| {
        if candidates.is_empty() { "" } else { candidates[0] }
    })
}

/// Like `classify_linear` but returns all candidates with their cosine
/// similarity scores, sorted descending.
pub fn classify_linear_scored(content: &str, candidates: &[&'static str]) -> Vec<(&'static str, f64)> {
    let bytes = content.as_bytes();
    let mut counts: HashMap<u32, u32> = HashMap::with_capacity(content.len() / 4);

    for &n in CHARGRAM_NS {
        if bytes.len() < n { continue; }
        for window in bytes.windows(n) {
            if window.len() > MAX_NGRAM_BYTES { continue; }
            // Skip n-grams spanning line boundaries (same filter as codegen).
            if window.contains(&b'\n') || window.contains(&b'\r') { continue; }
            // Only consider windows that are valid UTF-8.
            if let Ok(s) = std::str::from_utf8(window) {
                if let Some(&idx) = CHARGRAM_VOCABULARY.get(s) {
                    *counts.entry(idx).or_insert(0) += 1;
                }
            }
        }
    }

    // Build TF-ICF query vector and L2-normalize.
    let mut query: Vec<(u32, f64)> = counts
        .into_iter()
        .map(|(idx, freq)| {
            let capped = freq.min(TF_CAP);
            let tf = 1.0 + (capped as f64).ln();
            (idx, tf * CHARGRAM_ICF[idx as usize])
        })
        .collect();
    let norm = query.iter().map(|(_, v)| v * v).sum::<f64>().sqrt();
    if norm > 0.0 {
        for (_, v) in query.iter_mut() { *v /= norm; }
    }
    query.sort_by_key(|x| x.0);

    let score_lang = |lang: &'static str| -> Option<(&'static str, f64)> {
        CHARGRAM_CENTROIDS.get(lang).map(|c| (lang, cosine_dot(&query, c)))
    };

    let mut scored: Vec<(&'static str, f64)> = if candidates.is_empty() {
        CHARGRAM_CENTROIDS.keys().filter_map(|&lang| score_lang(lang)).collect()
    } else {
        candidates.iter().filter_map(|&lang| score_lang(lang)).collect()
    };

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if scored.is_empty() {
        if candidates.is_empty() {
            vec![("", f64::NEG_INFINITY)]
        } else {
            vec![(candidates[0], f64::NEG_INFINITY)]
        }
    } else {
        scored
    }
}

fn cosine_dot(a: &[(u32, f64)], b: &[(u32, f64)]) -> f64 {
    let (mut i, mut j) = (0, 0);
    let mut sum = 0.0;
    while i < a.len() && j < b.len() {
        if a[i].0 == b[j].0 { sum += a[i].1 * b[j].1; i += 1; j += 1; }
        else if a[i].0 < b[j].0 { i += 1; }
        else { j += 1; }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn classify_linear_rust() {
        let content = fs::read_to_string("samples/Rust/main.rs").unwrap();
        let candidates = vec!["C", "Rust"];
        assert_eq!(classify_linear(&content, &candidates), "Rust");
    }

    #[test]
    fn classify_linear_scored_descending() {
        let content = fs::read_to_string("samples/Rust/main.rs").unwrap();
        let candidates = vec!["C", "Rust"];
        let scores = classify_linear_scored(&content, &candidates);
        assert!(!scores.is_empty());
        for pair in scores.windows(2) {
            assert!(pair[0].1 >= pair[1].1);
        }
    }

    #[test]
    fn classify_linear_empty_candidates_returns_result() {
        let content = fs::read_to_string("samples/Rust/main.rs").unwrap();
        let result = classify_linear(&content, &[]);
        assert!(!result.is_empty());
    }
}
