include!("../codegen/taxonomy-config.rs");

/// Look up the broad family for a language name. Returns `""` if the language
/// is not in the taxonomy (implicitly belongs to the "General" family).
pub fn get_family(language: &str) -> &'static str {
    TAXONOMY.get(language).copied().unwrap_or("")
}

/// Filter a candidate list to those in the plurality family.
///
/// Counts how many candidates belong to each family (using the taxonomy),
/// then returns only the candidates in the most-voted family. Ties and
/// languages not present in the taxonomy retain the original list.
///
/// Never returns an empty slice — if filtering would produce an empty result
/// the original candidates are returned unchanged.
pub fn filter_to_majority_family(candidates: &[&'static str]) -> Vec<&'static str> {
    if candidates.len() <= 1 {
        return candidates.to_vec();
    }

    // Count family votes.
    let mut family_counts: Vec<(&'static str, usize)> = Vec::new();
    for &lang in candidates {
        let family = get_family(lang);
        if family.is_empty() {
            // "General" — don't force-include into any named family.
            continue;
        }
        if let Some(entry) = family_counts.iter_mut().find(|(f, _)| *f == family) {
            entry.1 += 1;
        } else {
            family_counts.push((family, 1));
        }
    }

    // Find the family with the most votes.
    let max_votes = family_counts.iter().map(|(_, c)| *c).max().unwrap_or(0);
    if max_votes == 0 {
        // No taxonomy entries for any candidate — return all.
        return candidates.to_vec();
    }

    // Collect winning families (there may be a tie).
    let winning_families: Vec<&str> = family_counts
        .iter()
        .filter(|(_, c)| *c == max_votes)
        .map(|(f, _)| *f)
        .collect();

    // If there's a tie, keep all candidates (ambiguous family).
    if winning_families.len() > 1 {
        return candidates.to_vec();
    }

    let winner = winning_families[0];
    let filtered: Vec<&'static str> = candidates
        .iter()
        .copied()
        .filter(|&lang| get_family(lang) == winner)
        .collect();

    if filtered.is_empty() {
        candidates.to_vec()
    } else {
        filtered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn javascript_is_webscript() {
        assert_eq!(get_family("JavaScript"), "WebScript");
    }

    #[test]
    fn rust_is_systems() {
        assert_eq!(get_family("Rust"), "Systems");
    }

    #[test]
    fn unknown_language_empty_family() {
        assert_eq!(get_family("NonExistentLanguage42"), "");
    }

    #[test]
    fn filter_web_vs_systems() {
        let candidates = vec!["JavaScript", "TypeScript", "C", "Rust"];
        // 2 WebScript vs 2 Systems — tie, return all
        let result = filter_to_majority_family(&candidates);
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn filter_majority_web() {
        let candidates = vec!["JavaScript", "TypeScript", "HTML", "C"];
        // 3 WebScript vs 1 Systems — WebScript wins
        let result = filter_to_majority_family(&candidates);
        assert!(result.contains(&"JavaScript"));
        assert!(result.contains(&"TypeScript"));
        assert!(result.contains(&"HTML"));
        assert!(!result.contains(&"C"));
    }

    #[test]
    fn single_candidate_unchanged() {
        let candidates = vec!["Python"];
        assert_eq!(filter_to_majority_family(&candidates), vec!["Python"]);
    }

    #[test]
    fn empty_candidates_unchanged() {
        let candidates: Vec<&'static str> = vec![];
        assert_eq!(filter_to_majority_family(&candidates), candidates);
    }

    #[test]
    fn all_general_returns_all() {
        // Languages not in taxonomy are "General" — should keep them all.
        let candidates = vec!["SomeObscureLanguage", "AnotherObscure"];
        let result = filter_to_majority_family(&candidates);
        assert_eq!(result.len(), 2);
    }
}
