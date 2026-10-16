//! Built-in Python spec: `#` comments, both quote kinds, triple-quoted  (constructor + curated oracle samples)

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

/// strings (their opening line — states are line-scoped, see the module
/// docs), f-string prefixes stay plain.
pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "python",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.python",
                words: &[
                    "assert", "await", "break", "case", "continue", "del", "elif", "else",
                    "except", "finally", "for", "if", "match", "pass", "raise", "return", "try",
                    "while", "with", "yield",
                ],
            },
            KeywordGroup {
                scope: "storage.type.function.python",
                words: &["def", "lambda"],
            },
            KeywordGroup {
                scope: "storage.type.class.python",
                words: &["class"],
            },
            KeywordGroup {
                scope: "keyword.other.python",
                words: &[
                    "and", "as", "async", "from", "global", "in", "is", "nonlocal", "not", "or",
                ],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
            interpolation: None,
                scope: "string.quoted.double.python",
            },
            StringKind {
                quote: '\'',
                escapes: true,
            interpolation: None,
                scope: "string.quoted.single.python",
            },
        ],
        spans: &[
            SpanKind {
                open: "\"\"\"",
                close: "\"\"\"",
                scope: "string.quoted.double.block.python",
            },
            SpanKind {
                open: "'''",
                close: "'''",
                scope: "string.quoted.single.block.python",
            },
            SpanKind {
                open: "#",
                close: "",
                scope: "comment.line.number-sign.python",
            },
        ],
        numbers: true,
        number_scope: "constant.numeric.python",
        ident_extra: &[],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "x = 1",
        "# comment\ndef main():\n    pass",
        "s = \"fn # let\"",
        "s = 'single # \"q\"'",
        "t = 'it\\'s'; u = 'a\\\\b'",
        "\"\"\"triple\nline\nstring\"\"\"",
        "'''single triple with \" inside'''",
        "\"\"\"a\"\"b\"\"\"",
        "''''''",
        "'''''",
        "f\"interp {x} end\"",
        "def f():\n    \"\"\"Docstring.\"\"\"\n    return 1",
        "'''unterminated triple",
        "\"unterminated string def class",
        "# unterminated comment def if",
        "\"\"\"\"quoted\"\"\"\"",
        "x = 42; y = 3.14; z = 0x1F; w = 1_000",
        "class C(Base):\n    def m(self):\n        return self",
        "if x and not y or z in w is v:\n    pass",
        "g = lambda x: x + 1",
        "import os.path as p\nfrom x import (a, b)",
        "async def f():\n    await g()",
        "match x:\n    case 1:\n        pass",
        "global g; nonlocal n; del x; assert y",
        "\ndef at_line_start\nx = at_line_start\n42 at_line_start",
        "a#b'c\"d'''e",
        "deffn def defx xdef _def def_",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
