//! Built-in Ruby spec: `"` strings with `#{}` interpolation, `'` strings,
//! `#` comments, numbers. Symbols (`:name`) and heredocs are deliberately
//! not modeled: a symbol opener would swallow the rest of the line, and
//! heredocs span lines by design (line-scoped states cannot follow them).

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "ruby",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.ruby",
                words: &[
                    "begin", "break", "case", "do", "else", "elsif", "end", "ensure", "for", "if",
                    "next", "redo", "rescue", "retry", "return", "super", "unless", "until",
                    "when", "while", "yield",
                ],
            },
            KeywordGroup {
                scope: "storage.type.ruby",
                words: &["class", "def", "module"],
            },
            KeywordGroup {
                scope: "keyword.other.ruby",
                words: &[
                    "and",
                    "attr_accessor",
                    "attr_reader",
                    "attr_writer",
                    "lambda",
                    "not",
                    "or",
                    "proc",
                    "raise",
                    "require",
                    "require_relative",
                    "self",
                ],
            },
            KeywordGroup {
                scope: "constant.language.ruby",
                words: &["true", "false", "nil"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: Some(("#{", '}')),
                scope: "string.quoted.double.ruby",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.ruby",
            },
        ],
        spans: &[SpanKind {
            open: "#",
            close: "",
            scope: "comment.line.number-sign.ruby",
        }],
        tags: &[],
        numbers: true,
        number_scope: "constant.numeric.ruby",
        ident_extra: &[],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "x = 1",
        "# comment\ndef foo",
        "s = \"double #{x} inside\"",
        "c = 'single'",
        "e = \"esc: \\\" ok\"",
        "'it\\'s' \"a\\\"b\"",
        "\"nested #{\"inner\"} out\"",
        "def initialize(name) = @name = name",
        "class Foo < Bar; end",
        "return nil if x == 42",
        "items.each { |i| puts i }",
        "\"\" ''",
        "endend end_ _end 1end",
        "defined? yield self",
        "if x\n  y = 2\nend",
        "\ndef at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
