//! Viterbi decoding over a sequence of per-region classifier scores.
//!
//! Applies a simple first-order HMM where the hidden state is the active
//! language for each region and transitions are penalized for label switches.
//! The contiguity bias suppresses single-window noise: a one-window anomaly
//! in a otherwise uniform Python file won't produce a spurious second label.
//!
//! Emission probability = classifier score for the region (clamped positive).
//! Transition cost = 0 for same label, -transition_penalty for different label
//! (all arithmetic in log-space so there's no underflow).

/// Viterbi-smooth a sequence of per-region score vectors.
///
/// `region_scores`: per-window scores from `region_engine::region_profile`.
///   Each element is `Vec<(&'static str, f64)>` sorted descending.
///
/// `transition_penalty`: log-space cost added when the most likely label
///   changes between adjacent windows. Higher values = stronger contiguity
///   bias. A value of 2.0 means a window needs to score ~7.4× better in the
///   new label than the old one to justify a switch.
///
/// Returns a `Vec<&'static str>` of one smoothed label per region, in order.
/// Returns an empty Vec if `region_scores` is empty.
pub fn viterbi_smooth(
    region_scores: &[Vec<(&'static str, f64)>],
    transition_penalty: f64,
) -> Vec<&'static str> {
    if region_scores.is_empty() {
        return vec![];
    }

    // Collect the union of all labels across all windows.
    let mut all_labels: Vec<&'static str> = Vec::new();
    for window in region_scores {
        for (lang, _) in window {
            if !all_labels.contains(lang) {
                all_labels.push(lang);
            }
        }
    }
    if all_labels.is_empty() {
        return vec![];
    }

    let n_windows = region_scores.len();
    let n_labels = all_labels.len();

    // Convert scores to log-space. Scores must be > 0; clamp to a small floor.
    let log_emission = |window_idx: usize, label_idx: usize| -> f64 {
        let label = all_labels[label_idx];
        let score = region_scores[window_idx]
            .iter()
            .find(|(l, _)| *l == label)
            .map(|(_, s)| *s)
            .unwrap_or(0.0);
        // Clamp to a small positive value before log to avoid -infinity.
        (score.max(1e-10)).ln()
    };

    // dp[j] = best log-probability ending in label j at the current window.
    // backptr[t][j] = label index at window t-1 that led to label j at window t.
    let mut dp = vec![f64::NEG_INFINITY; n_labels];
    let mut backptr: Vec<Vec<usize>> = Vec::with_capacity(n_windows);

    // Initialize with the first window.
    for j in 0..n_labels {
        dp[j] = log_emission(0, j);
    }
    backptr.push(vec![0usize; n_labels]); // placeholder for window 0

    // Forward pass.
    for t in 1..n_windows {
        let mut new_dp = vec![f64::NEG_INFINITY; n_labels];
        let mut bp = vec![0usize; n_labels];

        for j in 0..n_labels {
            let emit = log_emission(t, j);
            // Find the predecessor state that maximises dp[prev] + transition + emit.
            let (best_prev, best_score) = (0..n_labels)
                .map(|k| {
                    let transition = if k == j { 0.0 } else { -transition_penalty };
                    (k, dp[k] + transition + emit)
                })
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap();
            new_dp[j] = best_score;
            bp[j] = best_prev;
        }

        dp = new_dp;
        backptr.push(bp);
    }

    // Find the best final state.
    let best_final = (0..n_labels)
        .max_by(|&a, &b| dp[a].partial_cmp(&dp[b]).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(0);

    // Backtrack to build the label sequence.
    let mut path = vec![0usize; n_windows];
    path[n_windows - 1] = best_final;
    for t in (1..n_windows).rev() {
        path[t - 1] = backptr[t][path[t]];
    }

    path.into_iter().map(|j| all_labels[j]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(pairs: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
        pairs.to_vec()
    }

    #[test]
    fn empty_returns_empty() {
        assert!(viterbi_smooth(&[], 2.0).is_empty());
    }

    #[test]
    fn uniform_file_stays_uniform() {
        let windows: Vec<_> = (0..10)
            .map(|_| window(&[("Rust", 0.9), ("C", 0.1)]))
            .collect();
        let path = viterbi_smooth(&windows, 2.0);
        assert_eq!(path.len(), 10);
        assert!(path.iter().all(|&l| l == "Rust"), "expected all Rust, got {:?}", path);
    }

    #[test]
    fn noise_window_absorbed_into_dominant_label() {
        // 9 Rust windows, 1 noisy window in the middle
        let mut windows: Vec<_> = (0..10)
            .map(|i| {
                if i == 5 {
                    window(&[("C", 0.6), ("Rust", 0.4)]) // noise
                } else {
                    window(&[("Rust", 0.9), ("C", 0.1)])
                }
            })
            .collect();
        let path = viterbi_smooth(&windows, 2.0);
        // With penalty=2.0, a single noisy window should be smoothed to Rust.
        assert_eq!(path[5], "Rust", "noise window should be smoothed: {:?}", path);
        let _ = windows; // suppress warning
    }

    #[test]
    fn genuine_transition_is_preserved() {
        // 5 Rust windows then 5 Python windows with strong evidence.
        let windows: Vec<_> = (0..10)
            .map(|i| {
                if i < 5 {
                    window(&[("Rust", 0.95), ("Python", 0.05)])
                } else {
                    window(&[("Python", 0.95), ("Rust", 0.05)])
                }
            })
            .collect();
        let path = viterbi_smooth(&windows, 2.0);
        // First half should be Rust, second half Python.
        assert!(path[..5].iter().all(|&l| l == "Rust"), "first half: {:?}", &path[..5]);
        assert!(path[5..].iter().all(|&l| l == "Python"), "second half: {:?}", &path[5..]);
    }

    #[test]
    fn path_length_matches_input() {
        let windows: Vec<_> = (0..7)
            .map(|_| window(&[("JavaScript", 0.8), ("HTML", 0.2)]))
            .collect();
        let path = viterbi_smooth(&windows, 1.5);
        assert_eq!(path.len(), 7);
    }
}
