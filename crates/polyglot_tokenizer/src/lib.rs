pub mod linguist_tokens;
pub mod tokenizer;
pub use linguist_tokens::get_linguist_tokens;
pub use tokenizer::{Token, Tokenizer};

use linguist_tokens::{detect_obfuscation, detect_opener, OPENER_EMIT_COUNT};

/// Tokenize the content and return only the identifiers and symbols from the langauge.
///
/// If the content starts with a recognised opener (`<?php`, `<?hh`, `<?xml`,
/// optionally after a UTF-8 BOM), the iterator first yields an
/// `OPENER<?…>` pseudo-token `OPENER_EMIT_COUNT` times. The repetition
/// makes the marker dominate the per-token log-probability sum used by the
/// Bayes scorer in `src/detectors/classifier.rs`. The same trick is applied
/// in `get_linguist_tokens` for the TF-ICF path so both classifiers agree.
///
/// Obfuscation pseudo-tokens (`OBF:JSFUCK`, `OBF:FROMCHARCODE_LONG`, etc.)
/// are also emitted once each when the corresponding pattern is detected.
///
/// # Examples
/// ```
/// use polyglot_tokenizer;
/// let content = r#"let x = [5, "hello"];"#;
/// let tokens: Vec<&str> = polyglot_tokenizer::get_key_tokens(content).collect();
/// assert_eq!(tokens, vec!["let", "x", "=", "[", ",", "]", ";"]);
/// ```
pub fn get_key_tokens(content: &str) -> impl Iterator<Item = &str> {
    let opener_iter = detect_opener(content)
        .into_iter()
        .flat_map(|tok| std::iter::repeat(tok).take(OPENER_EMIT_COUNT));
    let obf_iter = detect_obfuscation(content).iter().copied();
    let regular = Tokenizer::new(content).tokens().filter_map(|t| match t {
        Token::Ident(t) | Token::Symbol(t) => Some(t),
        _ => None,
    });
    opener_iter.chain(obf_iter).chain(regular)
}
