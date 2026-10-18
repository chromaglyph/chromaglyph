//! Built-in JavaScript spec (covers JSX; TSX users want [`typescript`]).  (constructor + curated oracle samples)

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "javascript",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.js",
                followed_by: None,
                value: false,
                words: &[
                    "as",
                    "await",
                    "break",
                    "case",
                    "catch",
                    "continue",
                    "default",
                    "delete",
                    "do",
                    "else",
                    "export",
                    "finally",
                    "for",
                    "from",
                    "if",
                    "import",
                    "in",
                    "instanceof",
                    "new",
                    "return",
                    "switch",
                    "throw",
                    "try",
                    "typeof",
                    "while",
                    "with",
                    "debugger",
                    "yield",
                ],
            },
            KeywordGroup {
                scope: "storage.type.js",
                followed_by: None,
                value: false,
                words: &[
                    "var", "let", "const", "function", "class", "extends", "super", "static",
                    "get", "set", "async", "of",
                ],
            },
            KeywordGroup {
                scope: "constant.language.js",
                followed_by: None,
                value: false,
                words: &["true", "false", "null", "undefined", "this"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.double.js",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.js",
            },
            StringKind {
                quote: '`',
                escapes: true,
                interpolation: Some(("${", '}')),
                scope: "string.template.js",
            },
        ],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.js",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.block.js",
            },
        ],
        tags: &[],
        blocks: &[],
        numbers: true,
        number_scope: "constant.numeric.js",
        ident_extra: &['$'],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "let x = 1;",
        "// comment\nfunction f() {}",
        "const s = 'fn // let';",
        "// 'quoted' comment\nlet y;",
        "let s = \"a'b`c\";",
        "let q = 'it\\'s'; let r = \"a\\\"b\";",
        "let t = `x ${y} z`;",
        "let u = `tick \\` inside`;",
        "`multi\nline\ntemplate`",
        "'' \"\" ``",
        "'a\"b' \"c'd\" `e\"f'g`",
        "''''''",
        "//////",
        "/* block */ let x;",
        "/* multi\nline\ncomment */ const y;",
        "/* unterminated block let function",
        "a/*b'c*/d'e",
        "*/*/*/**/",
        "let a = 42; let b = 3.14; let c = 0x1F; let d = 1e5;",
        "1..toString() x1 12x 0b101 1_000",
        "letletlet let_ let1 1let",
        "async function* gen() { yield await x; }",
        "class A extends B { static m() { return this; } }",
        "import { a } from 'mod'; export default A;",
        "null undefined true false this",
        "\nfunction at_line_start\nlet at_line_start\n42 at_line_start",
        "let $x = $require('y'); $let let$ $let$",
        "let s = '\"' // '\"\n`//` \"'`",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
