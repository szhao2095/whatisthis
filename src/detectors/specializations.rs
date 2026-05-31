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

use lazy_static::lazy_static;
use pcre2::bytes::Regex;

/// How many leading bytes to scan for specialization markers. Markers in
/// practice live inside `<head>` or near the top of the document, so 8 KB
/// is more than enough.
const HEAD_BYTES: usize = 8192;

lazy_static! {
    /// HTML -> HTA when `<HTA:APPLICATION ...>` appears near the top.
    /// Case-insensitive because real-world droppers vary the casing.
    static ref HTML_TO_HTA: Regex = Regex::new(r"(?i)<HTA:APPLICATION\b").unwrap();

    /// HTML -> VBScript+HTMLDecoy when the file starts with an apostrophe-
    /// prefixed HTML DOCTYPE. The actual code is base64-chunked VBScript;
    /// every HTML-looking line at the top is commented out with `'` so the
    /// file inspects as HTML by simple sniffing while real execution stays
    /// VBScript. The leading `'<!DOCTYPE html>.` is a hard-to-spoof marker.
    static ref HTML_TO_VBSCRIPT_HTMLDECOY: Regex =
        Regex::new(r"(?i)^\s*'<!DOCTYPE\s+html").unwrap();
}

/// Apply post-classifier specialization to a detected language. Returns
/// the specialized language name if a rule fires, otherwise the input
/// language unchanged.
///
/// Two kinds of rules exist:
///
/// * **Base-conditional**: only fire when the classifier (or any prior
///   strategy) chose a specific base language. Use these when the marker
///   could plausibly appear in unrelated content (e.g. a JS string mentioning
///   `<HTA:APPLICATION>`); the classifier's verdict is the safety net.
///
/// * **Marker-authoritative**: fire regardless of classifier output because
///   the marker is so unambiguous at offset 0 that no other file shape
///   would naturally produce it (e.g. `^\s*'<!DOCTYPE html` — an apostrophe-
///   prefixed DOCTYPE is unique to the decoy pattern). These act like a
///   miniature magic-byte stage layered on top of the classifier.
pub fn apply_specialization(language: &'static str, content: &str) -> &'static str {
    let bytes = content.as_bytes();
    let head = &bytes[..bytes.len().min(HEAD_BYTES)];

    // Marker-authoritative rules: check first, regardless of input language.
    if HTML_TO_VBSCRIPT_HTMLDECOY.is_match(head).unwrap_or(false) {
        return "VBScript+HTMLDecoy";
    }

    // Base-conditional rules.
    if language == "HTML" && HTML_TO_HTA.is_match(head).unwrap_or(false) {
        return "HTA";
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
        // The 'HTA' string appears but not as the tag prefix.
        let s = "<html><body>I love HTA applications.</body></html>";
        assert_eq!(apply_specialization("HTML", s), "HTML");
    }

    #[test]
    fn non_html_languages_pass_through() {
        let s = "<HTA:APPLICATION /> doesn't matter, we're not HTML";
        assert_eq!(apply_specialization("Rust", s), "Rust");
        assert_eq!(apply_specialization("VBScript", s), "VBScript");
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
    fn leading_apostrophe_doctype_fires_regardless_of_base() {
        // Marker-authoritative: rule fires even if classifier picked
        // something other than HTML for the decoy file.
        let s = "'<!DOCTYPE html>.\r\n'<head>.\r\nrest of file";
        assert_eq!(apply_specialization("Slim", s), "VBScript+HTMLDecoy");
        assert_eq!(apply_specialization("VBScript", s), "VBScript+HTMLDecoy");
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
}
