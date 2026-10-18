//! Built-in Lua spec: `"` and `'` strings, `[[` long strings (a span —
//! line-scoped like everything else), `--` comments with the `--[[ ]]`
//! block variant, numbers.

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "lua",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.lua",
                words: &[
                    "break", "do", "else", "elseif", "end", "for", "goto", "if", "in", "repeat",
                    "return", "then", "until", "while",
                ],
            },
            KeywordGroup {
                scope: "storage.type.lua",
                words: &["function", "local"],
            },
            KeywordGroup {
                scope: "keyword.operator.lua",
                words: &["and", "not", "or"],
            },
            KeywordGroup {
                scope: "constant.language.lua",
                words: &["true", "false", "nil"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.double.lua",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.lua",
            },
        ],
        spans: &[
            SpanKind {
                open: "--[[",
                close: "]]",
                scope: "comment.block.lua",
            },
            SpanKind {
                open: "--",
                close: "",
                scope: "comment.line.double-dash.lua",
            },
            SpanKind {
                open: "[[",
                close: "]]",
                scope: "string.unquoted.long.lua",
            },
        ],
        numbers: true,
        number_scope: "constant.numeric.lua",
        ident_extra: &[],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "local x = 1",
        "-- comment\nlocal function f()",
        "s = \"double\"",
        "c = 'single'",
        "e = \"esc: \\\" ok\"",
        "long = [[no \\n escapes]]",
        "--[[ block ]]\nlocal y = 2",
        "--[[ multi\nline ]] local z",
        "for i = 1, 10 do print(i) end",
        "if not x and y or z then return nil end",
        "local t = { a = 1, b = \"two\" }",
        "\"\" '' [[]] --[[[]]]",
        "endend end_ _end 1end",
        "--[[ unterminated block function",
        "while true do break end",
        "\nlocal at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
