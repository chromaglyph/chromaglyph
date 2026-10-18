//! Built-in shell spec: `'` raw strings (no escapes — exactly like shell
//! single quotes), `"` strings with `${}` interpolation, `#` comments,
//! numbers. `$var` inside double quotes stays string body (the flat
//! engine colors `{$...}` holes only). The `#` opener colors mid-word
//! hashes too (`a#b`) — real shell treats those as literal text, but a
//! glyph FSM cannot see word starts; samples and docs say so.

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "shell",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.shell",
                words: &[
                    "break", "case", "continue", "do", "done", "elif", "else", "esac", "exit",
                    "fi", "for", "if", "in", "return", "select", "then", "until", "while",
                ],
            },
            KeywordGroup {
                scope: "storage.type.shell",
                words: &["function"],
            },
            KeywordGroup {
                scope: "storage.modifier.shell",
                words: &["declare", "export", "local", "readonly", "typeset", "unset"],
            },
            KeywordGroup {
                scope: "constant.language.shell",
                words: &["true", "false"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: Some(("${", '}')),
                scope: "string.quoted.double.shell",
            },
            StringKind {
                quote: '\'',
                escapes: false,
                interpolation: None,
                scope: "string.quoted.single.shell",
            },
        ],
        spans: &[SpanKind {
            open: "#",
            close: "",
            scope: "comment.line.number-sign.shell",
        }],
        tags: &[],
        numbers: true,
        number_scope: "constant.numeric.shell",
        ident_extra: &[],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "x=1",
        "# comment\nif [ -f x ]; then",
        "s=\"double\"",
        "c='single'",
        "e=\"esc: \\\" ok\"",
        "echo \"home is ${HOME} today\"",
        "raw='no \\n escapes'",
        "for f in *.txt; do echo \"$f\"; done",
        "function greet() { local name=$1; }",
        "case $x in a) ;; esac",
        "\"\" ''",
        "esacesac esac_ _esac 1esac",
        "exit 0 || true",
        "# unterminated comment if then",
        "\"unterminated string if then",
        "\nfunction at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
