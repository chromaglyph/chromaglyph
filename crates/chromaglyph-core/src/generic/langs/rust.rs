//! Built-in Rust spec: keywords per the reference (2018 edition) grammar.  (constructor + curated oracle samples)

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "rust",
        keywords: &[KeywordGroup {
            scope: "keyword.other.rust",
            followed_by: None,
            value: false,
            words: &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super",
                "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
        }],
        strings: &[StringKind {
            quote: '"',
            escapes: true,
            interpolation: None,
            scope: "string.quoted.double.rust",
        }],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.rust",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.line.double-slash.rust",
            },
        ],
        tags: &[],
        blocks: &[],
        numbers: true,
        number_scope: "constant.numeric.rust",
        ident_extra: &[],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "let x = 1;",
        "// comment\nfn main() {}",
        "let s = \"fn // let\";",
        "// \"quoted\" comment\nlet y;",
        "\"\"\"",
        "//////",
        "fnfn fn fnx xfn _fn fn_",
        "a\"b\"c\"d",
        "/\n// x\n/",
        "let s = \"\";",
        "let esc = \"quote: \\\" back: \\\\\";",
        "struct S; impl S { fn f() -> Self { Self } }",
        "/* block */ let x;",
        "/* multi\nline\ncomment */ let y;",
        "/* unterminated block let fn",
        "a/*b\"c*/d\"e",
        "*/*/*/**/",
        "let a = 42; let b = 3.14; let c = 0x1F_ff_u32; let d = 1_000;",
        "1.max(2).min(3)",
        "x1 12x a1b2 0b1010 1e5 1.2.3 42.",
        "\nfn at_line_start\nlet at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
