use std::collections::HashMap;

use super::ann::{simhash, simhash_shortlist};

include!("../codegen/tficf-model.rs");

const MAX_TOKEN_BYTES: usize = 32;

// Cap how much a single repeated token can contribute to the score. Without
// this, files padded with a fill character (e.g. obfuscated JS with
// thousands of `@` chars) let one low-ICF token dominate cosine similarity
// and bias the verdict toward whichever language's centroid happens to
// weight that token the most. With the cap, tf = 1 + log(min(freq, K)) so
// occurrences beyond K stop influencing the score. Must match the value
// used at training time in codegen.rs::train_tficf_classifier.
const TF_CAP: u32 = 100;

pub fn classify_tficf_scored(content: &str, candidates: &[&'static str]) -> Vec<(&'static str, f64)> {
    let mut counts: HashMap<u32, u32> = HashMap::with_capacity(content.len() / 8);
    for token in polyglot_tokenizer::get_linguist_tokens(content) {
        if token.len() > MAX_TOKEN_BYTES {
            continue;
        }
        if let Some(&idx) = TFICF_VOCABULARY.get(&*token) {
            *counts.entry(idx).or_insert(0) += 1;
        }
    }

    let mut query: Vec<(u32, f64)> = counts
        .into_iter()
        .map(|(idx, freq)| {
            let capped = freq.min(TF_CAP);
            let tf = 1.0 + (capped as f64).ln();
            (idx, tf * TFICF_ICF[idx as usize])
        })
        .collect();
    let norm = query.iter().map(|(_, v)| v * v).sum::<f64>().sqrt();
    if norm > 0.0 {
        for (_, v) in query.iter_mut() {
            *v /= norm;
        }
    }
    query.sort_by_key(|x| x.0);

    let score_lang = |lang: &'static str| -> Option<(&'static str, f64)> {
        TFICF_CENTROIDS.get(lang).map(|centroid| (lang, cosine_dot(&query, centroid)))
    };

    let mut scored: Vec<(&'static str, f64)> = if candidates.is_empty() {
        TFICF_CENTROIDS.keys().filter_map(|&lang| score_lang(lang)).collect()
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

pub fn classify_tficf(content: &str, candidates: &[&'static str]) -> &'static str {
    classify_tficf_scored(content, candidates)[0].0
}

/// Like `classify_tficf` but uses SimHash shortlisting to prune the centroid
/// search space before exact cosine computation. Only beneficial when
/// `candidates` is empty (open-world against all ~700 language centroids).
///
/// `shortlist_k`: maximum number of candidates to shortlist from the full
/// label space. Passing a small k (e.g. 16) makes this faster than exact
/// search, with a small recall cost.
pub fn classify_tficf_ann(content: &str, candidates: &[&'static str], shortlist_k: usize) -> &'static str {
    if !candidates.is_empty() {
        // Candidate set already small — just run exact scoring.
        return classify_tficf(content, candidates);
    }

    // Build TF-IDF query vector (same as classify_tficf_scored).
    let mut counts: HashMap<u32, u32> = HashMap::with_capacity(content.len() / 8);
    for token in polyglot_tokenizer::get_linguist_tokens(content) {
        if token.len() > MAX_TOKEN_BYTES { continue; }
        if let Some(&idx) = TFICF_VOCABULARY.get(&*token) {
            *counts.entry(idx).or_insert(0) += 1;
        }
    }
    let mut query: Vec<(u32, f64)> = counts.into_iter().map(|(idx, freq)| {
        let capped = freq.min(TF_CAP);
        let tf = 1.0 + (capped as f64).ln();
        (idx, tf * TFICF_ICF[idx as usize])
    }).collect();
    let norm = query.iter().map(|(_, v)| v * v).sum::<f64>().sqrt();
    if norm > 0.0 { for (_, v) in query.iter_mut() { *v /= norm; } }
    query.sort_by_key(|x| x.0);

    // Sketch the query vector.
    let query_sketch = simhash(&query);

    // Build centroid sketch pairs for shortlisting.
    let centroid_sketches: Vec<(&'static str, u64)> = TFICF_SKETCHES
        .entries()
        .map(|(&lang, &sketch)| (lang, sketch))
        .collect();

    let shortlist = simhash_shortlist(query_sketch, &centroid_sketches, shortlist_k);

    // Exact cosine over the shortlist.
    let mut best_lang = if shortlist.is_empty() { "" } else { shortlist[0] };
    let mut best_score = f64::NEG_INFINITY;
    for lang in &shortlist {
        if let Some(centroid) = TFICF_CENTROIDS.get(lang) {
            let score = cosine_dot(&query, centroid);
            if score > best_score {
                best_score = score;
                best_lang = lang;
            }
        }
    }
    best_lang
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_classify_tficf_scored_order() {
        let content = fs::read_to_string("samples/Rust/main.rs").unwrap();
        let candidates = vec!["C", "Rust"];
        let scores = classify_tficf_scored(content.as_str(), &candidates);
        assert_eq!(scores[0].0, "Rust");
        assert!(scores[0].1 >= scores[1].1, "scores must be sorted descending");
    }

    #[test]
    fn test_classify_tficf_scored_matches_classify_tficf() {
        let content = fs::read_to_string("samples/Rust/main.rs").unwrap();
        let candidates = vec!["C", "Rust"];
        let winner = classify_tficf(content.as_str(), &candidates);
        let scores = classify_tficf_scored(content.as_str(), &candidates);
        assert_eq!(winner, scores[0].0);
    }
}

fn cosine_dot(a: &[(u32, f64)], b: &[(u32, f64)]) -> f64 {
    let (mut i, mut j) = (0, 0);
    let mut sum = 0.0;
    while i < a.len() && j < b.len() {
        if a[i].0 == b[j].0 {
            sum += a[i].1 * b[j].1;
            i += 1;
            j += 1;
        } else if a[i].0 < b[j].0 {
            i += 1;
        } else {
            j += 1;
        }
    }
    sum
}
