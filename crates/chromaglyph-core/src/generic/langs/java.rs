//! Built-in Java spec: `"` strings, `'` char literals, both comment  (constructor + curated oracle samples)

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

/// kinds, declaration/modifier keyword groups.
pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "java",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.java",
                words: &[
                    "assert",
                    "break",
                    "case",
                    "catch",
                    "continue",
                    "do",
                    "else",
                    "finally",
                    "for",
                    "if",
                    "instanceof",
                    "return",
                    "switch",
                    "throw",
                    "throws",
                    "try",
                    "while",
                    "yield",
                ],
            },
            KeywordGroup {
                scope: "storage.type.java",
                words: &[
                    "boolean",
                    "byte",
                    "char",
                    "class",
                    "double",
                    "enum",
                    "float",
                    "int",
                    "interface",
                    "long",
                    "record",
                    "short",
                    "void",
                ],
            },
            KeywordGroup {
                scope: "storage.modifier.java",
                words: &[
                    "abstract",
                    "default",
                    "final",
                    "native",
                    "private",
                    "protected",
                    "public",
                    "sealed",
                    "static",
                    "strictfp",
                    "synchronized",
                    "transient",
                    "volatile",
                ],
            },
            KeywordGroup {
                scope: "keyword.other.java",
                words: &[
                    "extends",
                    "implements",
                    "import",
                    "new",
                    "package",
                    "super",
                    "this",
                ],
            },
            KeywordGroup {
                scope: "constant.language.java",
                words: &["true", "false", "null"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.double.java",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.java",
            },
        ],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.java",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.block.java",
            },
        ],
        numbers: true,
        number_scope: "constant.numeric.java",
        ident_extra: &[],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "int x = 1;",
        "// comment\npublic class A {}",
        "String s = \"double\";",
        "char c = 'x'; char q = '\\'';",
        "String e = \"esc: \\\" ok\";",
        "/* block */ private int y;",
        "/* multi\nline */ int z = 2;",
        "final var v = new ArrayList<>();",
        "if (x instanceof String) return null;",
        "for (int i = 0; i < 42; i++) continue;",
        "\"\" ''",
        "classclass class_ _class 1class",
        "interface I extends J, K {}",
        "switch (x) { case 1 -> break; }",
        "\nclass at_line_start\nint at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
