use crate::tokenizer::{Token, Tokenizer};

/// Emit Linguist-style tokens for classifier training/inference.
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
///
/// Numbers and string literals are dropped, same as `get_key_tokens`.
///
/// Known limitation: the base tokenizer in this crate has bugs around
/// multi-character block-comment delimiters (`/* */`, `<!-- -->`, `{- -}`,
/// `(* *)`) that cause them to leak through as individual symbol tokens
/// rather than `Token::BlockComment`. As a result, `COMMENT/*` and friends
/// are rarely emitted in practice; line-comment typing (`COMMENT#`,
/// `COMMENT//`, `COMMENT--`, `COMMENT%`) works as expected.
pub fn get_linguist_tokens(content: &str) -> Vec<String> {
    let content_base = content.as_ptr() as usize;
    let token_pos = |s: &str| -> usize { s.as_ptr() as usize - content_base };

    let raw: Vec<(Token, usize)> = Tokenizer::new(content)
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

    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let (t, pos) = (&raw[i].0, raw[i].1);
        match t {
            Token::Number(_) | Token::String(_, _, _) => {
                i += 1;
            }
            Token::LineComment(opener, body) => {
                // The base tokenizer captures `#` as the opener even when the
                // line starts with `#!` — recombine the `!` from the body so
                // we get a real `COMMENT#!` or shebang token.
                let starts_with_bang = body.starts_with('!');
                let effective_opener: String =
                    if *opener == "#" && starts_with_bang { "#!".into() } else { (*opener).into() };

                if effective_opener == "#!" && pos < first_newline {
                    let interp_src = &body[1..];
                    let interp = extract_interpreter(interp_src);
                    out.push(format!("SHEBANG#!{}", interp));
                } else {
                    out.push(format!("COMMENT{}", effective_opener));
                }
                i += 1;
            }
            Token::BlockComment(opener, _, _) => {
                out.push(format!("COMMENT{}", opener));
                i += 1;
            }
            Token::Ident(s) => {
                out.push((*s).to_string());
                i += 1;
            }
            Token::Symbol(s) => {
                // Sigil + adjacent ident → combine (no `#` because base
                // tokenizer commits `#` to line-comment parsing).
                if matches!(*s, "@" | "$" | ".") && i + 1 < raw.len() {
                    let next = &raw[i + 1];
                    if pos + s.len() == next.1 {
                        if let Token::Ident(ident_s) = &next.0 {
                            out.push(format!("{}{}", s, ident_s));
                            i += 2;
                            continue;
                        }
                    }
                }
                // Maximal-munch adjacent same-class symbol chars. Brackets
                // never coalesce — emit them individually. Coalescing only
                // applies to operator-class chars so JSFuck and other
                // bracket-heavy inputs stay tokenizable.
                if is_bracket(s) {
                    out.push((*s).to_string());
                    i += 1;
                    continue;
                }
                let mut combined = String::from(*s);
                let mut end_pos = pos + s.len();
                let mut j = i + 1;
                while j < raw.len() {
                    if let Token::Symbol(next_s) = &raw[j].0 {
                        if raw[j].1 == end_pos && !is_bracket(next_s) {
                            combined.push_str(next_s);
                            end_pos += next_s.len();
                            j += 1;
                            continue;
                        }
                    }
                    break;
                }
                out.push(combined);
                i = j;
            }
        }
    }

    out
}

fn is_bracket(s: &str) -> bool {
    matches!(s, "(" | ")" | "[" | "]" | "{" | "}")
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

    #[test]
    fn shebang_simple() {
        let toks = get_linguist_tokens("#!/usr/bin/python\nprint(1)\n");
        assert_eq!(toks[0], "SHEBANG#!python");
    }

    #[test]
    fn shebang_env() {
        let toks = get_linguist_tokens("#!/usr/bin/env python\nx = 1\n");
        assert_eq!(toks[0], "SHEBANG#!python");
    }

    #[test]
    fn shebang_env_with_flags() {
        let toks = get_linguist_tokens("#!/usr/bin/env -S python3 -u\n");
        assert!(toks[0].starts_with("SHEBANG#!"));
    }

    #[test]
    fn line_comments_typed() {
        let toks = get_linguist_tokens("// a\n# b\n-- c\n% d\n");
        assert!(toks.contains(&"COMMENT//".to_string()));
        assert!(toks.contains(&"COMMENT#".to_string()));
        assert!(toks.contains(&"COMMENT--".to_string()));
        assert!(toks.contains(&"COMMENT%".to_string()));
    }

    #[test]
    fn sigil_combines() {
        let toks = get_linguist_tokens("@foo $bar .baz");
        assert!(toks.contains(&"@foo".to_string()), "got {:?}", toks);
        assert!(toks.contains(&"$bar".to_string()), "got {:?}", toks);
        assert!(toks.contains(&".baz".to_string()), "got {:?}", toks);
    }

    #[test]
    fn dot_sigil_combines_after_ident() {
        let toks = get_linguist_tokens("foo.bar");
        assert!(toks.iter().any(|t| t == "foo"));
        assert!(toks.iter().any(|t| t == ".bar"));
    }

    #[test]
    fn multi_char_symbols() {
        let toks = get_linguist_tokens("a -> b => c != d <= e :: f");
        assert!(toks.contains(&"->".to_string()), "expected -> got {:?}", toks);
        assert!(toks.contains(&"=>".to_string()), "expected => got {:?}", toks);
        assert!(toks.contains(&"!=".to_string()), "expected != got {:?}", toks);
        assert!(toks.contains(&"<=".to_string()), "expected <= got {:?}", toks);
        assert!(toks.contains(&"::".to_string()), "expected :: got {:?}", toks);
    }

    #[test]
    fn numbers_and_strings_dropped() {
        let toks = get_linguist_tokens(r#"let x = 5; let s = "hi";"#);
        assert!(!toks.iter().any(|t| t == "5"));
        assert!(!toks.iter().any(|t| t == "hi"));
        assert!(toks.iter().any(|t| t == "let"));
    }

    #[test]
    fn separated_symbols_dont_combine() {
        let toks = get_linguist_tokens("a ! = b");
        assert!(!toks.contains(&"!=".to_string()));
        assert!(toks.contains(&"!".to_string()));
        assert!(toks.contains(&"=".to_string()));
    }
}
