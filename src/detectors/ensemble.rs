//! Calibrated ensemble fusion for hostile-mode detection.
//!
//! Combines scores from multiple evidence channels (structure hits, byte
//! statistics, Bayes, TF-ICF, and the char n-gram linear classifier) into
//! a single fused verdict. Supports abstention and ambiguity output when no
//! label clears the confidence threshold.

use super::entropy::ByteStats;
use super::structure::StructureHit;

include!("../codegen/fusion-config.rs");
use fusion_config::*;

/// All available evidence for one file (or chunk).
/// Fields are optional so the ensemble degrades gracefully when a backend
/// is disabled — only non-empty `Vec`s contribute to the fused score.
#[derive(Default)]
pub struct EvidenceBundle<'a> {
    pub structure_hits: Vec<StructureHit>,
    pub byte_stats: Option<ByteStats>,
    /// (language, score) pairs sorted descending, from classify_scored.
    pub bayes_scores: Vec<(&'static str, f64)>,
    /// (language, score) pairs sorted descending, from classify_tficf_scored.
    pub tficf_scores: Vec<(&'static str, f64)>,
    /// (language, score) pairs sorted descending, from classify_linear_scored.
    pub linear_scores: Vec<(&'static str, f64)>,
    /// Candidate set passed to the classifier stages. Empty = all languages.
    pub candidates: &'a [&'static str],
}

/// How aggressively to apply confidence thresholds.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ConfidenceMode {
    /// Require score >= THRESHOLD_HIGH to emit a label.
    High,
    /// Require score >= THRESHOLD_MED.
    Medium,
    /// Always return the argmax (never abstain). Same as default single-file behavior.
    BestGuess,
}

/// Fused detection verdict.
#[derive(Debug)]
pub enum FusedVerdict {
    /// One label with confidence score [0,1].
    Confident(&'static str, f64),
    /// Top two labels are too close to each other.
    Ambiguous(Vec<(&'static str, f64)>),
    /// No label clears the abstain threshold.
    Unknown,
}

/// Fuse all available evidence into a single verdict.
///
/// Signal normalization:
/// - Structure hits contribute a hard 1.0 score for their named format.
/// - Classifier scores are cosine similarities or log-prob sums; they are
///   min-max normalized over the candidates to [0,1] before weighting.
/// - The entropy gate: if entropy > ENTROPY_SUPPRESS and printable_ratio <
///   ENTROPY_PRINTABLE_MIN, text-classifier weights are zeroed out.
pub fn fuse(evidence: &EvidenceBundle<'_>, mode: ConfidenceMode) -> FusedVerdict {
    // ── Entropy gate ────────────────────────────────────────────────────────
    let text_suppressed = evidence.byte_stats.map_or(false, |s| {
        s.entropy > ENTROPY_SUPPRESS && s.printable_ratio < ENTROPY_PRINTABLE_MIN
    });

    // ── Collect all label candidates ────────────────────────────────────────
    let mut all_labels: Vec<&'static str> = Vec::new();
    let add_labels = |labels: &mut Vec<&'static str>, scores: &[(&'static str, f64)]| {
        for (lang, _) in scores {
            if !labels.contains(lang) {
                labels.push(lang);
            }
        }
    };
    for hit in &evidence.structure_hits {
        if !all_labels.contains(&hit.name) {
            all_labels.push(hit.name);
        }
    }
    add_labels(&mut all_labels, &evidence.bayes_scores);
    add_labels(&mut all_labels, &evidence.tficf_scores);
    add_labels(&mut all_labels, &evidence.linear_scores);
    if all_labels.is_empty() {
        return FusedVerdict::Unknown;
    }

    // ── Normalize each signal's scores to [0,1] ──────────────────────────────
    let norm_scores = |scores: &[(&'static str, f64)]| -> Vec<(&'static str, f64)> {
        if scores.is_empty() { return vec![]; }
        let min = scores.iter().map(|(_, s)| *s).fold(f64::INFINITY, f64::min);
        let max = scores.iter().map(|(_, s)| *s).fold(f64::NEG_INFINITY, f64::max);
        let range = max - min;
        if range <= 0.0 {
            scores.iter().map(|(l, _)| (*l, 1.0)).collect()
        } else {
            scores.iter().map(|(l, s)| (*l, (s - min) / range)).collect()
        }
    };

    let bayes_n  = norm_scores(&evidence.bayes_scores);
    let tficf_n  = norm_scores(&evidence.tficf_scores);
    let linear_n = norm_scores(&evidence.linear_scores);

    let lookup = |label: &str, scores: &[(&'static str, f64)]| -> f64 {
        scores.iter().find(|(l, _)| *l == label).map(|(_, s)| *s).unwrap_or(0.0)
    };

    // ── Compute fused score for each label ───────────────────────────────────
    let mut fused: Vec<(&'static str, f64)> = all_labels
        .iter()
        .map(|&label| {
            // Structure signal: 1.0 if any hit matches this label.
            let struct_score = if evidence.structure_hits.iter().any(|h| h.name == label) {
                1.0
            } else {
                0.0
            };

            let text_score = if text_suppressed {
                0.0
            } else {
                let b = lookup(label, &bayes_n)  * W_BAYES;
                let t = lookup(label, &tficf_n)  * W_TFICF;
                let l = lookup(label, &linear_n) * W_LINEAR;
                b + t + l
            };

            let score = struct_score * W_STRUCTURE + text_score;
            (label, score)
        })
        .collect();

    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if fused.is_empty() {
        return FusedVerdict::Unknown;
    }

    let best_score = fused[0].1;

    // ── Abstain check ───────────────────────────────────────────────────────
    let threshold = match mode {
        ConfidenceMode::High      => THRESHOLD_HIGH,
        ConfidenceMode::Medium    => THRESHOLD_MED,
        ConfidenceMode::BestGuess => ABSTAIN_BELOW,
    };

    if best_score < threshold && mode != ConfidenceMode::BestGuess {
        return FusedVerdict::Unknown;
    }

    // ── Ambiguity check ─────────────────────────────────────────────────────
    if fused.len() >= 2 {
        let second = fused[1].1;
        if best_score - second < AMBIGUITY_MARGIN {
            return FusedVerdict::Ambiguous(fused[..2].to_vec());
        }
    }

    FusedVerdict::Confident(fused[0].0, best_score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_evidence_returns_unknown() {
        let e = EvidenceBundle::default();
        match fuse(&e, ConfidenceMode::BestGuess) {
            FusedVerdict::Unknown => {}
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn strong_single_signal_returns_confident() {
        let e = EvidenceBundle {
            tficf_scores: vec![("Rust", 0.9), ("C", 0.1)],
            ..Default::default()
        };
        match fuse(&e, ConfidenceMode::BestGuess) {
            FusedVerdict::Confident("Rust", _) => {}
            other => panic!("expected Confident(Rust), got {:?}", other),
        }
    }

    #[test]
    fn high_entropy_suppresses_text_classifiers() {
        let e = EvidenceBundle {
            byte_stats: Some(ByteStats {
                entropy: 7.9,
                printable_ratio: 0.05,
                null_ratio: 0.1,
                hex_density: 0.3,
                base64_density: 0.3,
            }),
            tficf_scores: vec![("Rust", 0.95), ("C", 0.05)],
            ..Default::default()
        };
        // No structure hit → should return Unknown because text suppressed
        match fuse(&e, ConfidenceMode::High) {
            FusedVerdict::Unknown => {}
            other => panic!("expected Unknown when text suppressed, got {:?}", other),
        }
    }

    #[test]
    fn structure_hit_overrides_entropy_suppression() {
        let e = EvidenceBundle {
            byte_stats: Some(ByteStats {
                entropy: 7.9,
                printable_ratio: 0.05,
                null_ratio: 0.1,
                hex_density: 0.3,
                base64_density: 0.3,
            }),
            structure_hits: vec![StructureHit { name: "ELF", suppress_text_classifier: true }],
            ..Default::default()
        };
        match fuse(&e, ConfidenceMode::BestGuess) {
            FusedVerdict::Confident("ELF", _) => {}
            other => panic!("expected Confident(ELF), got {:?}", other),
        }
    }
}
