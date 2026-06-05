use crate::tokenizer::{Token, Tokenizer};
use std::borrow::Cow;

/// Times an `OPENER<?…>` pseudo-token is emitted at the start of a file that
/// begins with a recognised marker. The repetition exists to saturate the
/// classifier's term-frequency machinery so the opener dominates L2
/// normalisation (TF-ICF) or weighs heavily in the per-token log-prob sum
/// (Bayes). Set to `TFICF_TF_CAP` (100) — emitting more is silently capped.
///
/// TODO: replace with a per-token weight in the classifier so we don't need
/// to physically push 100 entries through the Vec/iterator.
pub(crate) const OPENER_EMIT_COUNT: usize = 100;

/// Emit Linguist-style tokens for classifier training/inference.
///
/// Returns `Vec<Cow<'a, str>>` to avoid allocating for the common case where
/// the output token is a slice of the input (raw idents, single-char symbols,
/// coalesced operator runs). Synthesized tokens (`SHEBANG#!…`, sigil idents)
/// and rare comment openers are owned `String`s.
///
/// Differences from `get_key_tokens`:
/// * Line and block comments become typed tokens like `COMMENT#`, `COMMENT//`,
///   `COMMENT/*`, `COMMENT<!--` etc., preserving the comment style without
///   leaking comment content.
/// * A `#!` line at the start of the file becomes `SHEBANG#!<interpreter>`,
///   where `<interpreter>` is the basename of the path or the binary after
///   `env`.
/// * Identifiers preceded immediately by `@`, `$`, or `.` are combined with
///   the sigil (`@var`, `.foo`). `#var` is not supported because the base
///   tokenizer treats `#` as a line-comment opener.
/// * Adjacent symbol characters with no intervening whitespace/token are
///   combined into a single token (`==`, `!=`, `->`, `=>`, `<<=`, etc.).
///   Brackets `()[]{}` are intentionally never coalesced; without this carve-
///   out, bracket-heavy inputs (e.g. JSFuck) collapse into one mega-token
///   that the classifier's per-token length cap then discards.
///
/// Numbers and string literals are dropped, same as `get_key_tokens`.
///
/// Known limitation: the base tokenizer in this crate has bugs around
/// multi-character block-comment delimiters (`/* */`, `<!-- -->`, `{- -}`,
/// `(* *)`) that cause them to leak through as individual symbol tokens
/// rather than `Token::BlockComment`. As a result, `COMMENT/*` and friends
/// are rarely emitted in practice; line-comment typing (`COMMENT#`,
/// `COMMENT//`, `COMMENT--`, `COMMENT%`) works as expected.
/// Scan content for structural obfuscation patterns and return a static
/// slice of matching pseudo-token names.  Each token is emitted once — the
/// high ICF weight comes from the tokens being rare in the training corpus,
/// not from repetition.
///
/// These pseudo-tokens are picked up by both the Bayes and TF-ICF trainers
/// when codegen is run, so they naturally acquire discriminative weights for
/// obfuscated variants without any changes to the classifier math.
pub(crate) fn detect_obfuscation(content: &str) -> &'static [&'static str] {
    // Bitmask: collect which pseudo-tokens fire, then return a pre-built slice.
    // Using a small array avoids heap allocation; at most 5 tokens fire.
    let bytes = content.as_bytes();
    let len = bytes.len();
    if len == 0 {
        return &[];
    }

    let mut flags: u8 = 0;

    // OBF:JSFUCK — JSFuck uses only []()!+ to encode any JavaScript.
    // Fire when those six chars make up ≥ 60% of the content.
    {
        let jsfuck_count = bytes.iter().filter(|&&b| matches!(b, b'[' | b']' | b'(' | b')' | b'!' | b'+')).count();
        if jsfuck_count * 10 >= len * 6 {
            flags |= 1 << 0;
        }
    }

    // OBF:FROMCHARCODE_LONG — String.fromCharCode with ≥8 comma-separated args.
    // Indicates numeric character-code decoding patterns.
    if contains_pattern(content, b"String.fromCharCode") && has_long_charcode_call(content) {
        flags |= 1 << 1;
    }

    // OBF:PS_ENCODED — PowerShell encoded-command indicators.
    if contains_pattern_ci(bytes, b"-enc") || contains_pattern_ci(bytes, b"-encodedcommand") || contains_pattern(content, b"FromBase64String") {
        flags |= 1 << 2;
    }

    // OBF:BATCH_CARET_HIGH — batch caret-escape density ≥ 5%.
    {
        let caret_count = bytes.iter().filter(|&&b| b == b'^').count();
        if caret_count * 20 >= len {
            flags |= 1 << 3;
        }
    }

    // OBF:HTML_SCRIPT_TAG — <script present beyond offset 0 (not the opener).
    // Indicates an HTML file with embedded scripts.
    if content.len() > 7 && contains_pattern_ci(&bytes[1..], b"<script") {
        flags |= 1 << 4;
    }

    // Map flags to a static pre-built slice to avoid allocation.
    match flags {
        0 => &[],
        1 => &["OBF:JSFUCK"],
        2 => &["OBF:FROMCHARCODE_LONG"],
        3 => &["OBF:JSFUCK", "OBF:FROMCHARCODE_LONG"],
        4 => &["OBF:PS_ENCODED"],
        5 => &["OBF:JSFUCK", "OBF:PS_ENCODED"],
        6 => &["OBF:FROMCHARCODE_LONG", "OBF:PS_ENCODED"],
        7 => &["OBF:JSFUCK", "OBF:FROMCHARCODE_LONG", "OBF:PS_ENCODED"],
        8 => &["OBF:BATCH_CARET_HIGH"],
        9 => &["OBF:JSFUCK", "OBF:BATCH_CARET_HIGH"],
        10 => &["OBF:FROMCHARCODE_LONG", "OBF:BATCH_CARET_HIGH"],
        11 => &["OBF:JSFUCK", "OBF:FROMCHARCODE_LONG", "OBF:BATCH_CARET_HIGH"],
        12 => &["OBF:PS_ENCODED", "OBF:BATCH_CARET_HIGH"],
        13 => &["OBF:JSFUCK", "OBF:PS_ENCODED", "OBF:BATCH_CARET_HIGH"],
        14 => &["OBF:FROMCHARCODE_LONG", "OBF:PS_ENCODED", "OBF:BATCH_CARET_HIGH"],
        15 => &["OBF:JSFUCK", "OBF:FROMCHARCODE_LONG", "OBF:PS_ENCODED", "OBF:BATCH_CARET_HIGH"],
        16 => &["OBF:HTML_SCRIPT_TAG"],
        17 => &["OBF:JSFUCK", "OBF:HTML_SCRIPT_TAG"],
        18 => &["OBF:FROMCHARCODE_LONG", "OBF:HTML_SCRIPT_TAG"],
        19 => &["OBF:JSFUCK", "OBF:FROMCHARCODE_LONG", "OBF:HTML_SCRIPT_TAG"],
        20 => &["OBF:PS_ENCODED", "OBF:HTML_SCRIPT_TAG"],
        21 => &["OBF:JSFUCK", "OBF:PS_ENCODED", "OBF:HTML_SCRIPT_TAG"],
        22 => &["OBF:FROMCHARCODE_LONG", "OBF:PS_ENCODED", "OBF:HTML_SCRIPT_TAG"],
        23 => &["OBF:JSFUCK", "OBF:FROMCHARCODE_LONG", "OBF:PS_ENCODED", "OBF:HTML_SCRIPT_TAG"],
        24 => &["OBF:BATCH_CARET_HIGH", "OBF:HTML_SCRIPT_TAG"],
        25 => &["OBF:JSFUCK", "OBF:BATCH_CARET_HIGH", "OBF:HTML_SCRIPT_TAG"],
        26 => &["OBF:FROMCHARCODE_LONG", "OBF:BATCH_CARET_HIGH", "OBF:HTML_SCRIPT_TAG"],
        27 => &["OBF:JSFUCK", "OBF:FROMCHARCODE_LONG", "OBF:BATCH_CARET_HIGH", "OBF:HTML_SCRIPT_TAG"],
        28 => &["OBF:PS_ENCODED", "OBF:BATCH_CARET_HIGH", "OBF:HTML_SCRIPT_TAG"],
        29 => &["OBF:JSFUCK", "OBF:PS_ENCODED", "OBF:BATCH_CARET_HIGH", "OBF:HTML_SCRIPT_TAG"],
        30 => &["OBF:FROMCHARCODE_LONG", "OBF:PS_ENCODED", "OBF:BATCH_CARET_HIGH", "OBF:HTML_SCRIPT_TAG"],
        _ => &["OBF:JSFUCK", "OBF:FROMCHARCODE_LONG", "OBF:PS_ENCODED", "OBF:BATCH_CARET_HIGH", "OBF:HTML_SCRIPT_TAG"],
    }
}

fn contains_pattern(content: &str, pattern: &[u8]) -> bool {
    content.as_bytes().windows(pattern.len()).any(|w| w == pattern)
}

fn contains_pattern_ci(bytes: &[u8], pattern: &[u8]) -> bool {
    if bytes.len() < pattern.len() {
        return false;
    }
    bytes.windows(pattern.len()).any(|w| {
        w.iter().zip(pattern).all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
    })
}

/// Check whether the content contains a String.fromCharCode call with ≥8
/// comma-separated numeric arguments (indicating a character-code decoder).
fn has_long_charcode_call(content: &str) -> bool {
    let mut search = content;
    while let Some(idx) = search.find("String.fromCharCode") {
        let after = &search[idx + 19..];
        // Count commas in the next 512 chars (one call's worth of arguments)
        let window = &after[..after.len().min(512)];
        let commas = window.bytes().take_while(|&b| b != b')').filter(|&b| b == b',').count();
        if commas >= 7 {
            return true;
        }
        search = &search[idx + 1..];
    }
    false
}

pub fn get_linguist_tokens<'a>(content: &'a str) -> Vec<Cow<'a, str>> {
    let content_base = content.as_ptr() as usize;
    let token_pos = |s: &str| -> usize { s.as_ptr() as usize - content_base };

    let opener = detect_opener(content);

    let raw: Vec<(Token<'a>, usize)> = Tokenizer::new(content)
        .tokens()
        .map(|t| {
            let pos = match &t {
                Token::Ident(s) | Token::Symbol(s) | Token::Number(s) => token_pos(s),
                Token::String(open, _, _)
                | Token::BlockComment(open, _, _)
                | Token::LineComment(open, _) => token_pos(open),
            };
            (t, pos)
        })
        .collect();

    let first_newline = content.find('\n').unwrap_or(content.len());

    // OPENER tokens are emitted many times to saturate the per-document TF cap
    // (TFICF_TF_CAP = 100). A single emission is washed out by L2 normalisation
    // when the rest of the file is dominated by another language's tokens
    // (e.g. PHP webshells whose body is mostly embedded HTML/JS). Saturating
    // the cap makes the marker a high-weight feature that cosine similarity
    // against the language centroid can latch onto.
    let obf_tokens = detect_obfuscation(content);
    let mut out: Vec<Cow<'a, str>> = Vec::with_capacity(raw.len() + OPENER_EMIT_COUNT + obf_tokens.len());
    if let Some(tok) = opener {
        for _ in 0..OPENER_EMIT_COUNT {
            out.push(Cow::Borrowed(tok));
        }
    }
    for tok in obf_tokens {
        out.push(Cow::Borrowed(tok));
    }
    let mut i = 0;
    while i < raw.len() {
        let (t, pos) = (&raw[i].0, raw[i].1);
        match t {
            Token::Number(_) => {
                i += 1;
            }
            Token::String(_, body, _) => {
                if let Some(shape) = string_shape(body) {
                    out.push(Cow::Borrowed(shape));
                }
                i += 1;
            }
            Token::LineComment(opener, body) => {
                let starts_with_bang = body.starts_with('!');
                if *opener == "#" && starts_with_bang && pos < first_newline {
                    let interp = extract_interpreter(&body[1..]);
                    let mut s = String::with_capacity(9 + interp.len());
                    s.push_str("SHEBANG#!");
                    s.push_str(&interp);
                    out.push(Cow::Owned(s));
                } else {
                    out.push(line_comment_token(opener, starts_with_bang));
                }
                i += 1;
            }
            Token::BlockComment(opener, _, _) => {
                out.push(block_comment_token(opener));
                i += 1;
            }
            Token::Ident(s) => {
                out.push(Cow::Borrowed(*s));
                i += 1;
            }
            Token::Symbol(s) => {
                // Sigil + adjacent ident → combine into one owned token.
                if matches!(*s, "@" | "$" | ".") && i + 1 < raw.len() {
                    let next = &raw[i + 1];
                    if pos + s.len() == next.1 {
                        if let Token::Ident(ident_s) = &next.0 {
                            let mut owned = String::with_capacity(s.len() + ident_s.len());
                            owned.push_str(s);
                            owned.push_str(ident_s);
                            out.push(Cow::Owned(owned));
                            i += 2;
                            continue;
                        }
                    }
                }
                // Brackets emit individually so bracket-heavy inputs stay
                // tokenizable.
                if is_bracket(s) {
                    out.push(Cow::Borrowed(*s));
                    i += 1;
                    continue;
                }
                // Maximal-munch adjacent non-bracket operator-class chars.
                // Because the run is contiguous in source, we borrow a slice
                // of `content` rather than allocating.
                let mut end_pos = pos + s.len();
                let mut j = i + 1;
                while j < raw.len() {
                    if let Token::Symbol(next_s) = &raw[j].0 {
                        if raw[j].1 == end_pos && !is_bracket(next_s) {
                            end_pos += next_s.len();
                            j += 1;
                            continue;
                        }
                    }
                    break;
                }
                out.push(Cow::Borrowed(&content[pos..end_pos]));
                i = j;
            }
        }
    }

    out
}

/// Recognise a small set of unambiguous, position-anchored file openers at
/// offset 0 and emit a synthetic `OPENER<marker>` pseudo-token. These give
/// the classifier a single high-ICF feature for cases where the rest of the
/// file is dominated by another language (e.g. PHP webshells that are
/// mostly embedded HTML/JS by token count).
///
/// Only a UTF-8 BOM is tolerated before the opener; otherwise it must be at
/// the very first byte. Matches are exact (case-sensitive) so that legit
/// content can't accidentally trip the marker.
///
/// Shared between `get_linguist_tokens` (TF-ICF path) and `get_key_tokens`
/// (Bayes path) so that both classifiers see the same opener signal.
pub(crate) fn detect_opener(content: &str) -> Option<&'static str> {
    let s = content.strip_prefix('\u{feff}').unwrap_or(content);
    if s.starts_with("<?php") {
        return Some("OPENER<?php");
    }
    if s.starts_with("<?hh") {
        return Some("OPENER<?hh");
    }
    if s.starts_with("<?xml") {
        return Some("OPENER<?xml");
    }
    // `<script language="VBScript">` (or any case variant of the language
    // attribute) at offset 0. Catches bare-script-fragment droppers whose
    // body is huge hex shellcode the tokenizer can't fully parse, leaving
    // them with only a handful of tokens that score poorly against any
    // language centroid. The case-insensitive scan is over a small window.
    if starts_with_ascii_ci(s, "<script") {
        let after = &s[7..];
        let win = &after[..after.len().min(64)].to_ascii_lowercase();
        if win.contains("vbscript") && !win.contains("javascript") {
            return Some("OPENER<script>VBScript");
        }
    }
    // `<!DOCTYPE html` at offset 0 (case-insensitive). Catches both HTML5
    // (`<!DOCTYPE html>`) and XHTML 1.0/HTML 4.01 PUBLIC variants. Files that
    // are real HTML but whose body is dominated by oddball tokens (asterisk-
    // separated obfuscated payloads, embedded long base64 strings, etc.)
    // can lose to adjacent centroids like Graphviz or GSP without this.
    if starts_with_ascii_ci(s, "<!DOCTYPE html") {
        return Some("OPENER<!DOCTYPE>html");
    }
    // Bare `<html` (case-insensitive) at offset 0, when followed by a tag-
    // terminator. Covers DOCTYPE-less HTML fragments. The terminator check
    // avoids matching `<htmltag>` or other unrelated identifiers.
    if starts_with_ascii_ci(s, "<html") {
        let after = s.as_bytes().get(5).copied();
        if matches!(after, Some(b'>') | Some(b' ') | Some(b'\n') | Some(b'\r') | Some(b'\t') | None) {
            return Some("OPENER<html>");
        }
    }
    None
}

fn starts_with_ascii_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len()
        && s.as_bytes()[..prefix.len()]
            .iter()
            .zip(prefix.bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
}

fn line_comment_token(opener: &str, starts_with_bang: bool) -> Cow<'static, str> {
    if opener == "#" && starts_with_bang {
        return Cow::Borrowed("COMMENT#!");
    }
    match opener {
        "//" => Cow::Borrowed("COMMENT//"),
        "///" => Cow::Borrowed("COMMENT///"),
        "#" => Cow::Borrowed("COMMENT#"),
        "##" => Cow::Borrowed("COMMENT##"),
        "--" => Cow::Borrowed("COMMENT--"),
        "%" => Cow::Borrowed("COMMENT%"),
        _ => {
            let mut s = String::with_capacity(7 + opener.len());
            s.push_str("COMMENT");
            s.push_str(opener);
            Cow::Owned(s)
        }
    }
}

fn block_comment_token(opener: &str) -> Cow<'static, str> {
    match opener {
        "/*" => Cow::Borrowed("COMMENT/*"),
        "/**" => Cow::Borrowed("COMMENT/**"),
        "<!--" => Cow::Borrowed("COMMENT<!--"),
        "{-" => Cow::Borrowed("COMMENT{-"),
        "(*" => Cow::Borrowed("COMMENT(*"),
        "\"\"\"" => Cow::Borrowed("COMMENT\"\"\""),
        "'''" => Cow::Borrowed("COMMENT'''"),
        _ => {
            let mut s = String::with_capacity(7 + opener.len());
            s.push_str("COMMENT");
            s.push_str(opener);
            Cow::Owned(s)
        }
    }
}

fn is_bracket(s: &str) -> bool {
    matches!(s, "(" | ")" | "[" | "]" | "{" | "}")
}

/// Classify a string literal body by simple character-class statistics and
/// return a synthetic pseudo-token name when the shape is distinctive
/// enough to be useful as a classifier feature. The body must already
/// have most strings (short, mixed) classified out via the `< 40` short-
/// circuit so we don't bloat the vocabulary with noise.
///
/// Costs O(len) and avoids allocation. Length cap matches MAX_TOKEN_BYTES
/// in classifier (32) since these names are short.
fn string_shape(body: &str) -> Option<&'static str> {
    const MIN_LEN: usize = 40;
    const LONG_LEN: usize = 256;
    if body.len() < MIN_LEN {
        return None;
    }

    let mut percent = 0u32;
    let mut hex = 0u32;
    let mut hex_letters = 0u32; // a-f/A-F only (excludes 0-9)
    let mut b64 = 0u32;
    let mut b64_letters = 0u32; // a-z/A-Z only (excludes digits/padding)
    let mut total = 0u32;
    for b in body.bytes() {
        total += 1;
        if b == b'%' {
            percent += 1;
        }
        let is_hex_digit = (b'0'..=b'9').contains(&b);
        let is_hex_letter = (b'a'..=b'f').contains(&b)
            || (b'A'..=b'F').contains(&b);
        if is_hex_digit || is_hex_letter {
            hex += 1;
        }
        if is_hex_letter {
            hex_letters += 1;
        }
        let is_b64_digit = (b'0'..=b'9').contains(&b);
        let is_b64_letter = (b'a'..=b'z').contains(&b)
            || (b'A'..=b'Z').contains(&b);
        if is_b64_letter || is_b64_digit || b == b'+' || b == b'/' || b == b'='
        {
            b64 += 1;
        }
        if is_b64_letter {
            b64_letters += 1;
        }
    }

    // URI-encoded payloads are the dominant case the classifier needs to
    // identify (document.write(unescape("%3C..."))-style).
    if percent * 10 >= total {
        return Some("STRING:URI-ENCODED");
    }
    // >=95% hex digits with at least some hex letters (a-f) → likely
    // \x-escape body or hex-encoded payload. Pure-digit strings (e.g. VBA
    // ChrW/Mid decimal codes) must not match; they aren't hex-encoded.
    if hex * 100 >= total * 95 && hex_letters > 0 {
        return Some("STRING:HEX");
    }
    // >=95% base64 alphabet with at least some letters → likely
    // base64-encoded blob. Pure-digit strings (e.g. VBA ChrW/Mid decimal
    // codes) must not match; they aren't base64.
    if b64 * 100 >= total * 95 && b64_letters > 0 {
        return Some("STRING:BASE64");
    }
    // Long but nothing distinctive — still useful as "this language tends
    // to contain very long strings"
    if body.len() >= LONG_LEN {
        return Some("STRING:LONG");
    }
    None
}

fn extract_interpreter(after_bang: &str) -> String {
    let line = after_bang.split(['\r', '\n']).next().unwrap_or("");
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let first_word: &str = trimmed.split_whitespace().next().unwrap_or("");
    let first_basename = first_word.rsplit('/').next().unwrap_or(first_word);

    if first_basename == "env" {
        for word in trimmed.split_whitespace().skip(1) {
            if word.contains('=') {
                continue;
            }
            if word.starts_with('-') {
                continue;
            }
            return word.rsplit('/').next().unwrap_or(word).to_string();
        }
        return String::new();
    }

    first_basename.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(toks: &[Cow<'_, str>], needle: &str) -> bool {
        toks.iter().any(|t| t == needle)
    }

    #[test]
    fn shebang_simple() {
        let toks = get_linguist_tokens("#!/usr/bin/python\nprint(1)\n");
        assert_eq!(&*toks[0], "SHEBANG#!python");
    }

    #[test]
    fn shebang_env() {
        let toks = get_linguist_tokens("#!/usr/bin/env python\nx = 1\n");
        assert_eq!(&*toks[0], "SHEBANG#!python");
    }

    #[test]
    fn shebang_env_with_flags() {
        let toks = get_linguist_tokens("#!/usr/bin/env -S python3 -u\n");
        assert!(toks[0].starts_with("SHEBANG#!"));
    }

    #[test]
    fn line_comments_typed() {
        let toks = get_linguist_tokens("// a\n# b\n-- c\n% d\n");
        assert!(contains(&toks, "COMMENT//"));
        assert!(contains(&toks, "COMMENT#"));
        assert!(contains(&toks, "COMMENT--"));
        assert!(contains(&toks, "COMMENT%"));
    }

    #[test]
    fn sigil_combines() {
        let toks = get_linguist_tokens("@foo $bar .baz");
        assert!(contains(&toks, "@foo"));
        assert!(contains(&toks, "$bar"));
        assert!(contains(&toks, ".baz"));
    }

    #[test]
    fn dot_sigil_combines_after_ident() {
        let toks = get_linguist_tokens("foo.bar");
        assert!(contains(&toks, "foo"));
        assert!(contains(&toks, ".bar"));
    }

    #[test]
    fn multi_char_symbols() {
        let toks = get_linguist_tokens("a -> b => c != d <= e :: f");
        assert!(contains(&toks, "->"));
        assert!(contains(&toks, "=>"));
        assert!(contains(&toks, "!="));
        assert!(contains(&toks, "<="));
        assert!(contains(&toks, "::"));
    }

    #[test]
    fn numbers_and_short_strings_dropped() {
        let toks = get_linguist_tokens(r#"let x = 5; let s = "hi";"#);
        assert!(!contains(&toks, "5"));
        assert!(!contains(&toks, "hi"));
        assert!(contains(&toks, "let"));
    }

    #[test]
    fn uri_encoded_string_pseudo_token() {
        // Lots of `%xx` patterns → URI-ENCODED label
        let s = "%3Cscript%3E%3Cdocument%3E%3Cwrite%3E%3Cunescape%3E%3Calert%3E";
        let src = format!("x(\"{}\")", s);
        let toks = get_linguist_tokens(&src);
        assert!(contains(&toks, "STRING:URI-ENCODED"), "got {:?}", toks);
    }

    #[test]
    fn long_string_pseudo_token() {
        // Use prose-like content with spaces and punctuation so it doesn't
        // match BASE64/HEX/URI-ENCODED shapes — only LONG should fire.
        let body = "hello world this is a long prose-like string. ".repeat(10);
        let src = format!("y(\"{}\")", body);
        let toks = get_linguist_tokens(&src);
        assert!(contains(&toks, "STRING:LONG"), "got {:?}", toks);
    }

    #[test]
    fn base64_string_pseudo_token() {
        let body = "U29mdHdhcmUgaXMgZWF0aW5nIHRoZSB3b3JsZC4gQWxsIHRoaXMgaXMganVzdCB0ZXh0Lg==";
        let src = format!("decode(\"{}\")", body);
        let toks = get_linguist_tokens(&src);
        assert!(contains(&toks, "STRING:BASE64"), "got {:?}", toks);
    }

    #[test]
    fn short_string_no_pseudo_token() {
        let toks = get_linguist_tokens(r#"y("hello world")"#);
        assert!(!toks.iter().any(|t| t.starts_with("STRING:")));
    }

    #[test]
    fn pure_digit_string_not_hex() {
        // VBA ChrW/Mid decimal-code strings: all digits, no hex letters →
        // must NOT get STRING:HEX (would misroute to JavaScript+HexString).
        let body = "039032098106110098102110010079112116105111110032069120112108".repeat(3);
        let src = format!("khqwh = \"{}\"", body);
        let toks = get_linguist_tokens(&src);
        assert!(
            !contains(&toks, "STRING:HEX"),
            "pure-digit string should not get STRING:HEX, got {:?}",
            toks
        );
    }

    #[test]
    fn hex_string_with_letters() {
        // Continuous hex-encoded payload (as it appears inside a string literal
        // after concatenation) with actual a-f letters → should get STRING:HEX.
        let body = "48656c6c6f20576f726c642e486572650s20616e6f746865722074657374".repeat(3);
        let src = format!("t = \"{}\"", body);
        let toks = get_linguist_tokens(&src);
        assert!(contains(&toks, "STRING:HEX"), "got {:?}", toks);
    }

    #[test]
    fn pure_digit_string_not_base64() {
        // VBA ChrW/Mid decimal-code strings: all digits, no letters →
        // must NOT get STRING:BASE64 (would misroute to Jupyter Notebook).
        let body = "039032098106110098102110010079112116105111110032069120112108".repeat(3);
        let src = format!("khqwh = \"{}\"", body);
        let toks = get_linguist_tokens(&src);
        assert!(
            !contains(&toks, "STRING:BASE64"),
            "pure-digit string should not get STRING:BASE64, got {:?}",
            toks
        );
    }

    #[test]
    fn opener_php_at_offset_0() {
        let toks = get_linguist_tokens("<?php\necho 1;\n");
        assert_eq!(&*toks[0], "OPENER<?php");
    }

    #[test]
    fn opener_php_with_bom() {
        let toks = get_linguist_tokens("\u{feff}<?php\necho 1;\n");
        assert_eq!(&*toks[0], "OPENER<?php");
    }

    #[test]
    fn opener_not_at_offset_0_does_not_match() {
        let toks = get_linguist_tokens("// comment\n<?php\necho 1;\n");
        assert!(!contains(&toks, "OPENER<?php"));
    }

    #[test]
    fn opener_xml() {
        let toks = get_linguist_tokens("<?xml version=\"1.0\"?>\n<root/>");
        assert_eq!(&*toks[0], "OPENER<?xml");
    }

    #[test]
    fn opener_script_vbscript_offset_0() {
        let toks = get_linguist_tokens("<script language=\"VBScript\">\nFunction Foo()\nEnd Function\n</script>");
        assert_eq!(&*toks[0], "OPENER<script>VBScript");
    }

    #[test]
    fn opener_script_vbscript_case_insensitive() {
        let toks = get_linguist_tokens("<SCRIPT LANGUAGE=\"vbscript\">\nDim x\n</SCRIPT>");
        assert_eq!(&*toks[0], "OPENER<script>VBScript");
    }

    #[test]
    fn opener_script_javascript_does_not_match() {
        let toks = get_linguist_tokens("<script language=\"JavaScript\">\nvar x = 1;\n</script>");
        assert!(!contains(&toks, "OPENER<script>VBScript"));
    }

    #[test]
    fn opener_doctype_html5() {
        let toks = get_linguist_tokens("<!DOCTYPE html>\n<html><body></body></html>");
        assert_eq!(&*toks[0], "OPENER<!DOCTYPE>html");
    }

    #[test]
    fn opener_doctype_xhtml() {
        let toks = get_linguist_tokens("<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\">\n<html/>");
        assert_eq!(&*toks[0], "OPENER<!DOCTYPE>html");
    }

    #[test]
    fn opener_doctype_lowercase() {
        let toks = get_linguist_tokens("<!doctype html>\n<html/>");
        assert_eq!(&*toks[0], "OPENER<!DOCTYPE>html");
    }

    #[test]
    fn opener_bare_html_tag() {
        let toks = get_linguist_tokens("<html>\n<head></head><body/></html>");
        assert_eq!(&*toks[0], "OPENER<html>");
    }

    #[test]
    fn opener_bare_html_with_attrs() {
        let toks = get_linguist_tokens("<html lang=\"en\">\n<body/></html>");
        assert_eq!(&*toks[0], "OPENER<html>");
    }

    #[test]
    fn opener_htmltag_does_not_match() {
        let toks = get_linguist_tokens("<htmltag foo>bar</htmltag>");
        assert!(!contains(&toks, "OPENER<html>"));
    }

    #[test]
    fn separated_symbols_dont_combine() {
        let toks = get_linguist_tokens("a ! = b");
        assert!(!contains(&toks, "!="));
        assert!(contains(&toks, "!"));
        assert!(contains(&toks, "="));
    }

    #[test]
    fn coalesced_operator_is_borrowed() {
        // For a contiguous operator run like `->`, the output should be a
        // Borrowed slice into the input (no heap allocation).
        let content = "a->b";
        let toks = get_linguist_tokens(content);
        let arrow = toks.iter().find(|t| t == &"->").unwrap();
        assert!(matches!(arrow, Cow::Borrowed(_)));
    }

    #[test]
    fn ident_is_borrowed() {
        let toks = get_linguist_tokens("foo bar");
        for t in &toks {
            assert!(matches!(t, Cow::Borrowed(_)), "unexpected owned: {:?}", t);
        }
    }

    // --- obfuscation pseudo-token tests ---

    #[test]
    fn jsfuck_dense_content_emits_token() {
        // 100 chars of only JSFuck characters (>60%)
        let jsfuck = "[]()!+".repeat(20);
        let toks = detect_obfuscation(&jsfuck);
        assert!(toks.contains(&"OBF:JSFUCK"), "expected OBF:JSFUCK, got {:?}", toks);
    }

    #[test]
    fn normal_source_no_jsfuck() {
        let src = "fn main() { let x = 42; println!(\"{}\", x); }";
        let toks = detect_obfuscation(src);
        assert!(!toks.contains(&"OBF:JSFUCK"));
    }

    #[test]
    fn fromcharcode_long_emits_token() {
        let src = "document.write(String.fromCharCode(60,115,99,114,105,112,116,62,97,108))";
        let toks = detect_obfuscation(src);
        assert!(toks.contains(&"OBF:FROMCHARCODE_LONG"), "got {:?}", toks);
    }

    #[test]
    fn fromcharcode_short_no_token() {
        let src = "String.fromCharCode(65,66,67)";
        let toks = detect_obfuscation(src);
        assert!(!toks.contains(&"OBF:FROMCHARCODE_LONG"));
    }

    #[test]
    fn ps_encoded_command_emits_token() {
        let src = "powershell.exe -EncodedCommand aABlAGwAbABvAA==";
        let toks = detect_obfuscation(src);
        assert!(toks.contains(&"OBF:PS_ENCODED"), "got {:?}", toks);
    }

    #[test]
    fn batch_caret_high_emits_token() {
        // 10 carets in a 100-char string = 10% density
        let src = "e^c^h^o^ ^h^e^l^l^o".repeat(5);
        let toks = detect_obfuscation(&src);
        assert!(toks.contains(&"OBF:BATCH_CARET_HIGH"), "got {:?}", toks);
    }

    #[test]
    fn html_script_tag_emits_token() {
        let src = "<!DOCTYPE html>\n<html><body><script>alert(1)</script></body></html>";
        let toks = detect_obfuscation(src);
        assert!(toks.contains(&"OBF:HTML_SCRIPT_TAG"), "got {:?}", toks);
    }

    #[test]
    fn empty_content_no_tokens() {
        let toks = detect_obfuscation("");
        assert!(toks.is_empty());
    }
}
