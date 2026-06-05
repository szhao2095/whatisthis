include!("../codegen/magic-config.rs");

/// A magic-byte match result.
#[derive(Debug, Clone, Copy)]
pub struct StructureHit {
    /// Short format name, e.g. "ELF", "PE", "ZIP".
    pub name: &'static str,
    /// When true, the file is a binary-only format and text classifiers
    /// should be suppressed (they have no training data for machine code).
    pub suppress_text_classifier: bool,
}

/// Check the first 32 bytes of `bytes` against the compiled MAGIC_RULES.
/// Returns all matching hits — a file can match multiple rules (e.g. ZIP
/// and OOXML both start with the ZIP magic bytes).
pub fn detect_structure(bytes: &[u8]) -> Vec<StructureHit> {
    MAGIC_RULES
        .iter()
        .filter(|rule| {
            let start = rule.offset;
            let end = start + rule.magic_bytes.len();
            bytes.len() >= end && &bytes[start..end] == rule.magic_bytes
        })
        .map(|rule| StructureHit {
            name: rule.name,
            suppress_text_classifier: rule.suppress_text_classifier,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elf_magic() {
        let bytes = b"\x7fELF\x02\x01\x01\x00rest_of_header";
        let hits = detect_structure(bytes);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "ELF");
        assert!(hits[0].suppress_text_classifier);
    }

    #[test]
    fn pe_magic() {
        let bytes = b"MZ\x90\x00\x03\x00\x00\x00";
        let hits = detect_structure(bytes);
        assert!(hits.iter().any(|h| h.name == "PE"));
    }

    #[test]
    fn pdf_magic() {
        let bytes = b"%PDF-1.4\n1 0 obj";
        let hits = detect_structure(bytes);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "PDF");
        assert!(!hits[0].suppress_text_classifier);
    }

    #[test]
    fn zip_magic() {
        let bytes = b"PK\x03\x04\x14\x00\x00\x00";
        let hits = detect_structure(bytes);
        assert!(hits.iter().any(|h| h.name == "ZIP"));
    }

    #[test]
    fn rust_source_no_hit() {
        let bytes = b"fn main() { println!(\"hello\"); }\n";
        let hits = detect_structure(bytes);
        assert!(hits.is_empty(), "expected no hits, got {:?}", hits.iter().map(|h| h.name).collect::<Vec<_>>());
    }

    #[test]
    fn empty_bytes_no_hit() {
        let hits = detect_structure(&[]);
        assert!(hits.is_empty());
    }

    #[test]
    fn png_magic() {
        let bytes = b"\x89PNG\r\n\x1a\nextra";
        let hits = detect_structure(bytes);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "PNG");
        assert!(hits[0].suppress_text_classifier);
    }
}
