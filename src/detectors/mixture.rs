//! EM mixture estimation over per-region classifier scores.
//!
//! Treats a file as a mixture of K language "components". Given per-region
//! score vectors from the region engine, runs Expectation-Maximization to
//! estimate file-level mixture weights π[l] for each language l.
//!
//! The E-step computes responsibilities (soft assignments of each window to
//! each language), and the M-step updates the mixing proportions. After
//! convergence the top `max_labels` languages by π weight are returned.
//!
//! This is especially useful for polyglot or wrapper files where a simple
//! single-verdict classifier returns the dominant language while a small
//! injected payload goes undetected.

/// Run EM mixture estimation.
///
/// `region_scores`: per-window score vectors (each `Vec` is `(lang, score)`
///   sorted descending). From `region_engine::region_profile`.
///
/// `max_labels`: maximum number of labels to include in the result.
///
/// `iters`: number of EM iterations (10 is typically enough for convergence).
///
/// Returns a `Vec<(&'static str, f64)>` of `(language, mixture_weight)`
/// sorted descending by weight, truncated to `max_labels`.
pub fn em_mixture(
    region_scores: &[Vec<(&'static str, f64)>],
    max_labels: usize,
    iters: usize,
) -> Vec<(&'static str, f64)> {
    if region_scores.is_empty() || max_labels == 0 {
        return vec![];
    }

    // Collect all unique languages across all windows.
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

    // Initialize mixture weights uniformly.
    let mut pi: Vec<f64> = vec![1.0 / n_labels as f64; n_labels];

    // Look up the score for a label in a window's score list.
    // Use 0.0 as a floor to avoid zero-probability issues.
    let get_score = |window: &Vec<(&'static str, f64)>, label: &str| -> f64 {
        window
            .iter()
            .find(|(l, _)| *l == label)
            .map(|(_, s)| s.max(0.0))
            .unwrap_or(0.0)
            + 1e-10  // small constant to avoid exact zero
    };

    let mut responsibilities = vec![vec![0.0f64; n_labels]; n_windows];

    for _iter in 0..iters {
        // E-step: compute responsibilities r[i, l] ∝ π[l] * score(window_i, l)
        for (i, window) in region_scores.iter().enumerate() {
            let mut row_sum = 0.0;
            for (j, &label) in all_labels.iter().enumerate() {
                let r = pi[j] * get_score(window, label);
                responsibilities[i][j] = r;
                row_sum += r;
            }
            // Normalize each row.
            if row_sum > 0.0 {
                for j in 0..n_labels {
                    responsibilities[i][j] /= row_sum;
                }
            } else {
                // Degenerate window: assign uniform responsibility.
                for j in 0..n_labels {
                    responsibilities[i][j] = 1.0 / n_labels as f64;
                }
            }
        }

        // M-step: update π[l] = mean_i r[i, l]
        for j in 0..n_labels {
            pi[j] = responsibilities.iter().map(|row| row[j]).sum::<f64>()
                / n_windows as f64;
        }
    }

    // Build result sorted by mixture weight descending.
    let mut result: Vec<(&'static str, f64)> = all_labels
        .iter()
        .enumerate()
        .map(|(j, &label)| (label, pi[j]))
        .collect();
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    result.truncate(max_labels);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scores(pairs: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
        pairs.to_vec()
    }

    #[test]
    fn empty_input_returns_empty() {
        let result = em_mixture(&[], 3, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn single_uniform_file_dominant_label() {
        // All windows strongly prefer "Rust"
        let windows: Vec<Vec<_>> = (0..10)
            .map(|_| scores(&[("Rust", 0.95), ("C", 0.05)]))
            .collect();
        let result = em_mixture(&windows, 3, 10);
        assert!(!result.is_empty());
        assert_eq!(result[0].0, "Rust");
        assert!(result[0].1 > 0.7, "Rust weight should dominate: {}", result[0].1);
    }

    #[test]
    fn mixed_file_shows_two_languages() {
        // 8 windows of HTML, 4 windows of JavaScript
        let mut windows: Vec<Vec<_>> = (0..8)
            .map(|_| scores(&[("HTML", 0.9), ("JavaScript", 0.1)]))
            .collect();
        windows.extend(
            (0..4).map(|_| scores(&[("JavaScript", 0.9), ("HTML", 0.1)]))
        );
        let result = em_mixture(&windows, 3, 15);
        assert!(result.len() >= 2);
        // Both HTML and JavaScript should appear
        assert!(result.iter().any(|(l, _)| *l == "HTML"));
        assert!(result.iter().any(|(l, _)| *l == "JavaScript"));
    }

    #[test]
    fn mixture_weights_sum_to_one() {
        let windows: Vec<Vec<_>> = vec![
            scores(&[("Rust", 0.8), ("C", 0.2)]),
            scores(&[("C", 0.7), ("Rust", 0.3)]),
        ];
        let result = em_mixture(&windows, 10, 10);
        let total: f64 = result.iter().map(|(_, w)| *w).sum();
        assert!((total - 1.0).abs() < 1e-6, "weights sum = {}", total);
    }

    #[test]
    fn result_sorted_descending() {
        let windows: Vec<Vec<_>> = (0..10)
            .map(|i| {
                if i < 7 {
                    scores(&[("HTML", 0.9), ("JavaScript", 0.1)])
                } else {
                    scores(&[("JavaScript", 0.8), ("HTML", 0.2)])
                }
            })
            .collect();
        let result = em_mixture(&windows, 3, 10);
        for pair in result.windows(2) {
            assert!(pair[0].1 >= pair[1].1);
        }
    }

    #[test]
    fn max_labels_respected() {
        let windows: Vec<Vec<_>> = vec![
            scores(&[("A", 0.5), ("B", 0.3), ("C", 0.2)]),
            scores(&[("B", 0.4), ("A", 0.4), ("C", 0.2)]),
        ];
        let result = em_mixture(&windows, 2, 10);
        assert!(result.len() <= 2);
    }
}
