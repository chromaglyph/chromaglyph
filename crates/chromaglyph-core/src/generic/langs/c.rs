//! Built-in C spec: `"` strings, `'` char literals, both comment kinds,
//! control/type/modifier keyword groups, preprocessor directive words.

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "c",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.c",
                words: &[
                    "break", "case", "continue", "default", "do", "else", "for", "goto", "if",
                    "return", "switch", "while",
                ],
            },
            KeywordGroup {
                scope: "storage.type.c",
                words: &[
                    "bool", "char", "double", "enum", "float", "int", "long", "short", "signed",
                    "struct", "typedef", "union", "unsigned", "void",
                ],
            },
            KeywordGroup {
                scope: "storage.modifier.c",
                words: &[
                    "auto", "const", "extern", "inline", "register", "restrict", "static",
                    "volatile",
                ],
            },
            // preprocessor directive words. The `#` is a plain glyph to the
            // font (not part of the identifier), so it stays uncolored and
            // the word itself colors — purple like an editor's directives.
            // These words color wherever they appear as identifiers;
            // `error`/`warning`/`line` were left out on purpose (plausible
            // C identifiers)
            KeywordGroup {
                scope: "keyword.control.import.c",
                words: &[
                    "define", "elif", "endif", "ifdef", "ifndef", "include", "pragma", "undef",
                ],
            },
            // `sizeof` renders blue like in VS Code; keyword.operator.word
            // resolves to the default foreground in dark-plus (= uncolored)
            KeywordGroup {
                scope: "keyword.other.c",
                words: &["sizeof"],
            },
            KeywordGroup {
                scope: "constant.language.c",
                words: &["false", "NULL", "true"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.double.c",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.c",
            },
        ],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.c",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.block.c",
            },
        ],
        numbers: true,
        number_scope: "constant.numeric.c",
        ident_extra: &[],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "int x = 1;",
        "// comment\nint main(void) {",
        "char *s = \"double\";",
        "char c = 'x'; char q = '\\'';",
        "char *e = \"esc: \\\" ok\";",
        "/* block */ static int y;",
        "/* multi\nline */ int z = 2;",
        "#include <stdio.h>",
        "#define MAX 42",
        "if (x != NULL) return -1;",
        "for (int i = 0; i < 42; i++) continue;",
        "\"\" ''",
        "intint int_ _int 1int",
        "struct P { int x; };",
        "switch (x) { default: break; }",
        "size_t n = sizeof(struct P);",
        "\nint at_line_start\nreturn at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
