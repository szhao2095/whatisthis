//! Post-classifier specialization (layered detection).
//!
//! Some languages are conceptually base-language + marker tag — e.g. HTA is
//! HTML with `<HTA:APPLICATION>`. Modelling them as flat sibling classes of
//! their base in the classifier leads to centroid overlap: real
//! base-language files get pulled toward the variant centroid because the
//! variant's training corpus is structurally a superset of the base.
//!
//! Specializations run after the classifier (or any other strategy) and
//! upgrade `base -> specialization` when an unambiguous marker is present
//! in the file head. The variant keeps its language-info entry but has no
//! classifier centroid (its `samples/` directory is empty/missing), so the
//! classifier never returns it directly — only the specialization rule does.
//!
//! Rules are declared in `specializations.yml` at the repo root and compiled
//! into `src/codegen/specializations-config.rs` by `cargo run --bin codegen`.
//! Adding a new specialization is a YAML edit + codegen + rebuild — no Rust
//! source changes required.

use lazy_static::lazy_static;
use pcre2::bytes::Regex;

include!("../codegen/specializations-config.rs");

/// How many leading bytes to scan for specialization markers. Markers in
/// practice live inside `<head>` or near the top of the document, so 8 KB
/// is more than enough.
const HEAD_BYTES: usize = 8192;

lazy_static! {
    /// Patterns compiled once on first use. Order mirrors the generated
    /// `SPECIALIZATIONS` table: marker-authoritative (no `base`) entries
    /// first, base-conditional ones after.
    static ref COMPILED: Vec<(&'static SpecializationRule, Regex)> = SPECIALIZATIONS
        .iter()
        .map(|rule| {
            let regex = Regex::new(rule.pattern).expect("specializations: pattern compiled at codegen but failed at runtime");
            (rule, regex)
        })
        .collect();
}

/// Apply post-classifier specialization to a detected language. Returns
/// the specialized language name if a rule fires, otherwise the input
/// language unchanged.
///
/// Two kinds of rules exist:
///
/// * **Base-conditional** (`base: <SomeLanguage>` in the YAML): only fires
///   when the prior detection stage chose that base. Use these when the
///   marker could plausibly appear in unrelated content; the classifier's
///   verdict is the safety net.
///
/// * **Marker-authoritative** (no `base` in the YAML): fires regardless of
///   prior stage because the offset-0 marker is so unambiguous that no
///   other file shape would naturally produce it. Effectively a miniature
///   magic-byte stage layered on top of the classifier.
pub fn apply_specialization(language: &'static str, content: &str) -> &'static str {
    let bytes = content.as_bytes();
    let head = &bytes[..bytes.len().min(HEAD_BYTES)];

    for (rule, regex) in COMPILED.iter() {
        if let Some(required_base) = rule.base {
            if language != required_base {
                continue;
            }
        }
        if regex.is_match(head).unwrap_or(false) {
            return rule.variant;
        }
    }
    language
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_with_hta_application_upgrades() {
        let s = "<html><head><title>X</title><HTA:APPLICATION ID=\"foo\" /></head></html>";
        assert_eq!(apply_specialization("HTML", s), "HTA");
    }

    #[test]
    fn html_with_hta_application_case_insensitive() {
        let s = "<html><head><hta:application id=foo></head></html>";
        assert_eq!(apply_specialization("HTML", s), "HTA");
    }

    #[test]
    fn plain_html_stays_html() {
        let s = "<!DOCTYPE html><html><body><p>hello</p></body></html>";
        assert_eq!(apply_specialization("HTML", s), "HTML");
    }

    #[test]
    fn html_mentioning_hta_word_does_not_match() {
        let s = "<html><body>I love HTA applications.</body></html>";
        assert_eq!(apply_specialization("HTML", s), "HTML");
    }

    #[test]
    fn hta_application_marker_fires_regardless_of_base() {
        // <HTA:APPLICATION> is a vendor-prefixed namespace tag essentially
        // unique to HTA. The rule is marker-authoritative, so it upgrades
        // even when the classifier picked something other than HTML.
        let s = "<head><HTA:APPLICATION /></head>";
        assert_eq!(apply_specialization("HTML+Django", s), "HTA");
        assert_eq!(apply_specialization("Handlebars", s), "HTA");
        assert_eq!(apply_specialization("Tea", s), "HTA");
    }

    #[test]
    fn html_with_leading_apostrophe_doctype_upgrades_to_vbscript_htmldecoy() {
        let s = "'<!DOCTYPE html>.\r\n'<html lang=\"en-US\">.\r\n'<head>.\r\n";
        assert_eq!(apply_specialization("HTML", s), "VBScript+HTMLDecoy");
    }

    #[test]
    fn html_with_leading_apostrophe_doctype_case_insensitive() {
        let s = "  '<!doctype HTML>.\r\n";
        assert_eq!(apply_specialization("HTML", s), "VBScript+HTMLDecoy");
    }

    #[test]
    fn html_with_normal_doctype_does_not_upgrade() {
        let s = "<!DOCTYPE html>\n<html><body>plain page</body></html>";
        assert_eq!(apply_specialization("HTML", s), "HTML");
    }

    #[test]
    fn html_mentioning_doctype_in_body_does_not_upgrade() {
        let s = "<!DOCTYPE html>\n<p>To start, write '<!DOCTYPE html>' as the first line.</p>";
        assert_eq!(apply_specialization("HTML", s), "HTML");
    }

    #[test]
    fn leading_apostrophe_doctype_fires_regardless_of_base() {
        // Marker-authoritative: rule fires even if classifier picked
        // something other than HTML for the decoy file.
        let s = "'<!DOCTYPE html>.\r\n'<head>.\r\nrest of file";
        assert_eq!(apply_specialization("Slim", s), "VBScript+HTMLDecoy");
        assert_eq!(apply_specialization("VBScript", s), "VBScript+HTMLDecoy");
        assert_eq!(apply_specialization("HTML", s), "VBScript+HTMLDecoy");
    }
}
