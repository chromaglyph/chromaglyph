//! Built-in Go spec: `"` strings, backtick raw strings (no escapes — a  (constructor + curated oracle samples)

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

/// backslash never suppresses the closer), `'` rune literals, both
/// comment kinds.
pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "go",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.go",
                followed_by: None,
                value: false,
                words: &[
                    "break",
                    "case",
                    "continue",
                    "default",
                    "defer",
                    "else",
                    "fallthrough",
                    "for",
                    "go",
                    "goto",
                    "if",
                    "return",
                    "select",
                    "switch",
                ],
            },
            KeywordGroup {
                scope: "keyword.other.go",
                followed_by: None,
                value: false,
                words: &[
                    "chan",
                    "const",
                    "func",
                    "import",
                    "interface",
                    "map",
                    "package",
                    "range",
                    "struct",
                    "type",
                    "var",
                ],
            },
            KeywordGroup {
                scope: "constant.language.go",
                followed_by: None,
                value: false,
                words: &["true", "false", "nil", "iota"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.double.go",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.go",
            },
            StringKind {
                quote: '`',
                escapes: false,
                interpolation: None,
                scope: "string.quoted.raw.go",
            },
        ],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.go",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.block.go",
            },
        ],
        tags: &[],
        blocks: &[],
        numbers: true,
        number_scope: "constant.numeric.go",
        ident_extra: &[],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "x := 1",
        "// comment\nfunc main() {}",
        "s := \"double\" + `raw` + 'r'",
        "q := \"esc: \\\" ok\"",
        "r := `raw \\ no escape`",
        "b := `ends with backslash \\`",
        "c := '\\\\''",
        "/* block */ var x int",
        "/* multi\nline */ y := 2",
        "`multi\nline\nraw`",
        "\"\" '' ``",
        "funcfunc func_ _func 1func",
        "if err != nil {\n    return nil\n}",
        "type T struct{ X int }",
        "m := map[string]int{}",
        "\nfunc at_line_start\nx = at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
