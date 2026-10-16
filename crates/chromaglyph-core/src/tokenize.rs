//! M1 walking-skeleton tokenizer: a small hardcoded lexer good enough to
//! exercise the compile/verify pipeline on Rust-ish code.
//!
//! M2 replaces this with syntect (TextMate grammars); M4 may add tree-sitter.
//! The only contract downstream code relies on: tokens are sorted,
//! non-overlapping, and cover every byte of the block text.

use crate::spec::Token;

pub const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

/// Tokenize `text` for `lang`. Unknown languages fall back to `plain`
/// (strings, numbers and line comments only; every identifier punctuation).
pub fn tokenize<'a>(text: &'a str, lang: &str) -> Vec<Token<'a>> {
    let keywords: &[&str] = match lang {
        "rust" | "rs" => RUST_KEYWORDS,
        _ => &[],
    };
    let allow_comments = matches!(lang, "rust" | "rs" | "plain");
    let allow_strings = true;

    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0usize;

    let push = |tokens: &mut Vec<Token>, start: usize, end: usize, scope: &'static str| {
        debug_assert!(end > start);
        tokens.push(Token { start, end, scope });
    };

    while i < bytes.len() {
        let b = bytes[i];
        let rest = &text[i..];
        // whitespace runs
        if b.is_ascii_whitespace() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            push(&mut tokens, start, i, "whitespace");
        }
        // line comments
        else if allow_comments && rest.starts_with("//") {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            push(&mut tokens, start, i, "comment");
        }
        // block comments (non-nesting, fine for a demo lexer)
        else if allow_comments && rest.starts_with("/*") {
            let start = i;
            let mut j = i + 2;
            while j < bytes.len() && !text[j..].starts_with("*/") {
                j += 1;
            }
            i = (j + 2).min(bytes.len());
            push(&mut tokens, start, i, "comment");
        }
        // string literal, "..." with backslash escapes
        else if allow_strings && b == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' => i = (i + 2).min(bytes.len()),
                    b'"' => {
                        i += 1;
                        break;
                    }
                    _ => i += 1,
                }
            }
            push(&mut tokens, start, i, "string");
        }
        // char literal, '...' with escapes (careful not to eat lifetimes:
        // only treat as char literal when a closing quote follows shortly)
        else if allow_strings && b == b'\'' && closes_char_literal(&text[i..]) {
            let start = i;
            i += 1;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' => i = (i + 2).min(bytes.len()),
                    b'\'' => {
                        i += 1;
                        break;
                    }
                    _ => i += 1,
                }
            }
            push(&mut tokens, start, i, "string");
        }
        // number literal: digits, underscores, exponent/suffix letters
        else if b.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            push(&mut tokens, start, i, "number");
        }
        // identifier / keyword
        else if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];
            let scope = if keywords.contains(&word) {
                "keyword"
            } else {
                "identifier"
            };
            push(&mut tokens, start, i, scope);
        }
        // anything else (ASCII punctuation, non-ASCII chars): one char per token
        else {
            let start = i;
            i += text[start..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(1);
            push(&mut tokens, start, i, "punctuation");
        }
    }

    debug_assert_eq!(
        tokens.first().map(|t| t.start),
        Some(0),
        "tokens must cover the text from byte 0"
    );
    tokens
}

/// Heuristic: does a `'` at the start of `s` begin a char literal
/// (`'a'`, `'\n'`, `'\u{1F600}'`) rather than a lifetime (`'a`)?
fn closes_char_literal(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 1;
    while i < bytes.len() && i <= 8 {
        match bytes[i] {
            b'\\' => i += 2,
            b'\'' => return true,
            _ if bytes[i].is_ascii_whitespace() => return false,
            _ => i += 1,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_all_bytes() {
        let text = "fn main() { let s = \"hi \\\\\" there\"; } // done\n";
        let tokens = tokenize(text, "rust");
        assert_eq!(tokens.first().unwrap().start, 0);
        assert_eq!(tokens.last().unwrap().end, text.len());
        for pair in tokens.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "tokens must be contiguous");
        }
    }

    #[test]
    fn recognizes_scopes() {
        let tokens = tokenize("fn x // c", "rust");
        let scopes: Vec<&str> = tokens.iter().map(|t| t.scope).collect();
        assert_eq!(
            scopes,
            vec![
                "keyword",
                "whitespace",
                "identifier",
                "whitespace",
                "comment"
            ]
        );
        assert_eq!(tokens[0].end - tokens[0].start, 2);
    }

    #[test]
    fn lifetime_is_not_char_literal() {
        let tokens = tokenize("&'static str", "rust");
        assert_eq!(tokens[1].scope, "punctuation"); // the quote, not a string
    }
}
