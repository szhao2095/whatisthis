//! Approximate nearest-neighbor shortlisting via SimHash.
//!
//! For large label spaces, exact cosine similarity against all centroids can
//! become the bottleneck. SimHash provides O(1) candidate shortlisting:
//! each centroid and query vector are sketched into a 64-bit integer using
//! random hyperplane projections. Hamming distance on the sketches approximates
//! angular distance (cosine dissimilarity), so the k nearest centroids by
//! Hamming distance are likely to be the k nearest by exact cosine.
//!
//! At current corpus scale (~700 languages, sparse vectors), exact cosine
//! already runs in microseconds — this module is useful when the label count
//! or window count grows materially larger.

/// Build a 64-bit SimHash sketch for a sparse vector `v` (sorted by index).
/// Uses a fixed set of 64 pseudo-random hyperplane seeds derived from the
/// index — no heap allocation, no external randomness.
pub fn simhash(v: &[(u32, f64)]) -> u64 {
    if v.is_empty() {
        return 0;
    }
    let mut bits = 0u64;
    for bit in 0u64..64 {
        // Accumulate the signed projection of v onto hyperplane `bit`.
        // Hyperplane normal: h[i] = sign(hash(i, bit)) ∈ {-1, +1}.
        let projection: f64 = v.iter().map(|(idx, val)| {
            let h = hash_coord(*idx, bit);
            *val * h
        }).sum();
        if projection >= 0.0 {
            bits |= 1u64 << bit;
        }
    }
    bits
}

/// Return the top-k centroid names sorted by Hamming distance to `query_sketch`.
///
/// `centroid_sketches`: slice of (language_name, sketch) pairs.
/// Returns up to `k` language names, closest first.
pub fn simhash_shortlist<'a>(
    query_sketch: u64,
    centroid_sketches: &'a [(&'static str, u64)],
    k: usize,
) -> Vec<&'static str> {
    if centroid_sketches.is_empty() || k == 0 {
        return vec![];
    }
    let mut scored: Vec<(&'static str, u32)> = centroid_sketches
        .iter()
        .map(|(lang, sketch)| (*lang, (query_sketch ^ sketch).count_ones()))
        .collect();
    scored.sort_by_key(|(_, hamming)| *hamming);
    scored.into_iter().take(k).map(|(lang, _)| lang).collect()
}

/// Pseudo-random ±1 projection of dimension `coord` for bit `bit`.
/// Uses a simple mixing hash to avoid storing an explicit projection matrix.
/// Sign is stable — same input always produces same output.
fn hash_coord(coord: u32, bit: u64) -> f64 {
    // Mix coord and bit with an FNV-1a-inspired step.
    let mut h: u64 = 14695981039346656037u64;
    h ^= coord as u64;
    h = h.wrapping_mul(1099511628211);
    h ^= bit;
    h = h.wrapping_mul(1099511628211);
    // Use the high bit as sign.
    if h & (1u64 << 63) != 0 { 1.0 } else { -1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_vector_sketch_zero() {
        assert_eq!(simhash(&[]), 0);
    }

    #[test]
    fn identical_vectors_identical_sketches() {
        let v = vec![(0u32, 1.0), (1u32, 0.5), (3u32, 0.25)];
        assert_eq!(simhash(&v), simhash(&v));
    }

    #[test]
    fn similar_vectors_close_hamming() {
        // Two vectors that share most of their mass.
        let v1 = vec![(0u32, 0.9), (1u32, 0.8), (2u32, 0.7)];
        let v2 = vec![(0u32, 0.95), (1u32, 0.75), (2u32, 0.72)];
        let s1 = simhash(&v1);
        let s2 = simhash(&v2);
        let hamming = (s1 ^ s2).count_ones();
        assert!(hamming <= 20, "expected close sketches, hamming={}", hamming);
    }

    #[test]
    fn shortlist_returns_at_most_k() {
        let centroids = vec![("Rust", 0u64), ("C", 15u64), ("Python", 255u64)];
        let result = simhash_shortlist(0u64, &centroids, 2);
        assert!(result.len() <= 2);
    }

    #[test]
    fn shortlist_empty_returns_empty() {
        let result = simhash_shortlist(0u64, &[], 5);
        assert!(result.is_empty());
    }

    #[test]
    fn shortlist_closest_first() {
        // query=0, centroids: A=0 (hamming 0), B=!0=all-ones (hamming 64)
        let centroids = vec![("far", u64::MAX), ("near", 0u64)];
        let result = simhash_shortlist(0u64, &centroids, 2);
        assert_eq!(result[0], "near");
        assert_eq!(result[1], "far");
    }
}
