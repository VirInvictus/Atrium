// SPDX-License-Identifier: MIT
//! Tokenizer for the search expression language.
//!
//! Splits an input string into a stream of [`Token`]s the parser
//! consumes. Whitespace separates tokens (except inside quoted
//! values); operators (`(`, `)`, `:`, `<=`, etc.) are their own
//! tokens; bare words become [`Token::Bareword`] and are sense-
//! disambiguated by the parser based on position.
//!
//! The lexer doesn't do semantic validation — it doesn't know
//! whether `tga:foo` is a typo or a future field operator; it just
//! produces `Bareword("tga")`, `Colon`, `Bareword("foo")` for the
//! parser to figure out.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A bare word — `tag`, `errand`, `today`, `Q3plans`. Joined
    /// runs of word characters; the parser interprets shape from
    /// position (left of `:` = field name, right of `:` = value, etc.)
    Bareword(String),
    /// A double-quoted string with escapes resolved. `"x y"` →
    /// `QuotedString("x y")`. `\"` and `\\` are the only escapes.
    QuotedString(String),
    /// `:` separator between a field name and its value.
    Colon,
    /// `(` left paren — opens a sub-expression group.
    LParen,
    /// `)` right paren.
    RParen,
    /// `=` equals — comparison or exact-match prefix.
    Eq,
    /// `!=` not-equals.
    Ne,
    /// `<` less-than.
    Lt,
    /// `<=` less-or-equal.
    Le,
    /// `>` greater-than.
    Gt,
    /// `>=` greater-or-equal.
    Ge,
    /// `~` regex-match prefix.
    Tilde,
    /// `..` inclusive range separator.
    DotDot,
    /// `!` NOT prefix shorthand.
    Bang,
}

/// Scan the input string into a vector of tokens. Garbage-in →
/// best-effort: malformed quoted strings produce a `QuotedString`
/// containing the raw input through end-of-string; unknown bytes are
/// treated as part of bare words.
pub fn lex(input: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        // Whitespace separates tokens.
        if (b as char).is_whitespace() {
            i += 1;
            continue;
        }

        // Quoted strings.
        if b == b'"' {
            i += 1;
            let mut value = String::new();
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    match bytes[i + 1] {
                        b'"' => {
                            value.push('"');
                            i += 2;
                        }
                        b'\\' => {
                            value.push('\\');
                            i += 2;
                        }
                        other => {
                            // Unknown escape — keep the backslash literal.
                            value.push('\\');
                            value.push(other as char);
                            i += 2;
                        }
                    }
                } else {
                    value.push(bytes[i] as char);
                    i += 1;
                }
            }
            // Skip the closing quote (or end-of-input).
            if i < bytes.len() {
                i += 1;
            }
            out.push(Token::QuotedString(value));
            continue;
        }

        // Multi-byte operators first.
        if i + 1 < bytes.len() {
            let pair = &bytes[i..i + 2];
            match pair {
                b"<=" => {
                    out.push(Token::Le);
                    i += 2;
                    continue;
                }
                b">=" => {
                    out.push(Token::Ge);
                    i += 2;
                    continue;
                }
                b"!=" => {
                    out.push(Token::Ne);
                    i += 2;
                    continue;
                }
                b".." => {
                    out.push(Token::DotDot);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }

        // Single-byte operators.
        match b {
            b':' => {
                out.push(Token::Colon);
                i += 1;
                continue;
            }
            b'(' => {
                out.push(Token::LParen);
                i += 1;
                continue;
            }
            b')' => {
                out.push(Token::RParen);
                i += 1;
                continue;
            }
            b'=' => {
                out.push(Token::Eq);
                i += 1;
                continue;
            }
            b'<' => {
                out.push(Token::Lt);
                i += 1;
                continue;
            }
            b'>' => {
                out.push(Token::Gt);
                i += 1;
                continue;
            }
            b'~' => {
                out.push(Token::Tilde);
                i += 1;
                continue;
            }
            b'!' => {
                out.push(Token::Bang);
                i += 1;
                continue;
            }
            _ => {}
        }

        // Bareword — accumulate until whitespace or a delimiter.
        let start = i;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if c.is_whitespace() {
                break;
            }
            if matches!(bytes[i], b':' | b'(' | b')' | b'"' | b'~' | b'!') {
                break;
            }
            // Multi-char operators that can split a bareword.
            if matches!(bytes[i], b'<' | b'>' | b'=') {
                break;
            }
            // `..` splits, but a single `.` in `2026-05-01` doesn't.
            // The DotDot match above handles consumption when we hit
            // it at the start of a token; if a `.` occurs *inside* a
            // bareword we keep it, and the next-char `.` triggers a
            // bareword break naturally because of the LParen/RParen
            // logic above. We disambiguate `.` followed by `.` as a
            // separator at the start of a token only.
            if bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1] == b'.' {
                break;
            }
            i += 1;
        }
        if i > start {
            out.push(Token::Bareword(input[start..i].to_string()));
        } else {
            // Defensive: should not happen given the matches above
            // but skip a stuck byte rather than infinite loop.
            i += 1;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_str(s: &str) -> Vec<Token> {
        lex(s)
    }

    #[test]
    fn empty_input_no_tokens() {
        assert!(lex_str("").is_empty());
        assert!(lex_str("   ").is_empty());
    }

    #[test]
    fn single_bareword() {
        assert_eq!(lex_str("milk"), vec![Token::Bareword("milk".into())]);
    }

    #[test]
    fn key_value_pair() {
        assert_eq!(
            lex_str("tag:work"),
            vec![
                Token::Bareword("tag".into()),
                Token::Colon,
                Token::Bareword("work".into())
            ]
        );
    }

    #[test]
    fn quoted_string_with_spaces() {
        assert_eq!(
            lex_str(r#""work focus""#),
            vec![Token::QuotedString("work focus".into())]
        );
    }

    #[test]
    fn quoted_string_escape_quote_and_backslash() {
        assert_eq!(
            lex_str(r#""he said \"hi\" \\ end""#),
            vec![Token::QuotedString(r#"he said "hi" \ end"#.into())]
        );
    }

    #[test]
    fn comparison_operators() {
        assert_eq!(
            lex_str("due:>2026-05-01"),
            vec![
                Token::Bareword("due".into()),
                Token::Colon,
                Token::Gt,
                Token::Bareword("2026-05-01".into())
            ]
        );
        assert_eq!(
            lex_str("estimated:>=30"),
            vec![
                Token::Bareword("estimated".into()),
                Token::Colon,
                Token::Ge,
                Token::Bareword("30".into())
            ]
        );
        assert_eq!(
            lex_str("title:!=draft"),
            vec![
                Token::Bareword("title".into()),
                Token::Colon,
                Token::Ne,
                Token::Bareword("draft".into())
            ]
        );
    }

    #[test]
    fn range_token() {
        assert_eq!(
            lex_str("due:2026-05-01..2026-05-31"),
            vec![
                Token::Bareword("due".into()),
                Token::Colon,
                Token::Bareword("2026-05-01".into()),
                Token::DotDot,
                Token::Bareword("2026-05-31".into())
            ]
        );
    }

    #[test]
    fn parens_and_boolean() {
        assert_eq!(
            lex_str("(tag:work AND NOT done) OR tag:home"),
            vec![
                Token::LParen,
                Token::Bareword("tag".into()),
                Token::Colon,
                Token::Bareword("work".into()),
                Token::Bareword("AND".into()),
                Token::Bareword("NOT".into()),
                Token::Bareword("done".into()),
                Token::RParen,
                Token::Bareword("OR".into()),
                Token::Bareword("tag".into()),
                Token::Colon,
                Token::Bareword("home".into()),
            ]
        );
    }

    #[test]
    fn bang_prefix_split_from_bareword() {
        assert_eq!(
            lex_str("!tag:work"),
            vec![
                Token::Bang,
                Token::Bareword("tag".into()),
                Token::Colon,
                Token::Bareword("work".into())
            ]
        );
    }

    #[test]
    fn tilde_regex_marker() {
        assert_eq!(
            lex_str("tag:~mystery"),
            vec![
                Token::Bareword("tag".into()),
                Token::Colon,
                Token::Tilde,
                Token::Bareword("mystery".into())
            ]
        );
    }
}
