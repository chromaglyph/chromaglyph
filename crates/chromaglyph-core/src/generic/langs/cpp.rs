//! Built-in C++ spec: C's lexical core plus class/template/namespace
//! keyword groups. Two C++ constructs are deliberately not modeled (the
//! glyph FSM cannot see them): raw strings `R"(...)"` — `R` is a plain
//! identifier, so the quote behaves like a normal escaped string (visually
//! right unless the raw body contains a bare quote, whose closer delimiter
//! is user-chosen and unknowable to the font) — and digit separators
//! `1'000`, where the `'` opens a char-literal state like any other quote.

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "cpp",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.c++",
                words: &[
                    "break", "case", "catch", "continue", "default", "do", "else", "for", "goto",
                    "if", "return", "switch", "throw", "try", "while",
                ],
            },
            KeywordGroup {
                scope: "storage.type.c++",
                words: &[
                    "auto", "bool", "char", "char16_t", "char32_t", "char8_t", "class", "decltype",
                    "double", "enum", "float", "int", "long", "short", "signed", "struct",
                    "typename", "union", "unsigned", "void", "wchar_t",
                ],
            },
            KeywordGroup {
                scope: "storage.modifier.c++",
                words: &[
                    "const",
                    "constexpr",
                    "extern",
                    "final",
                    "inline",
                    "mutable",
                    "override",
                    "private",
                    "protected",
                    "public",
                    "static",
                    "virtual",
                    "volatile",
                ],
            },
            KeywordGroup {
                scope: "keyword.other.c++",
                words: &[
                    "delete",
                    "namespace",
                    "new",
                    "operator",
                    "sizeof",
                    "template",
                    "this",
                    "using",
                ],
            },
            // preprocessor directive words, same trade-off as the C spec:
            // the `#` glyph stays plain, the word colors purple
            KeywordGroup {
                scope: "keyword.control.import.c++",
                words: &[
                    "define", "elif", "endif", "ifdef", "ifndef", "include", "pragma", "undef",
                ],
            },
            KeywordGroup {
                scope: "constant.language.c++",
                words: &["false", "NULL", "nullptr", "true"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.double.c++",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.c++",
            },
        ],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.c++",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.block.c++",
            },
        ],
        tags: &[],
        numbers: true,
        number_scope: "constant.numeric.c++",
        ident_extra: &[],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "int x = 1;",
        "// comment\nint main() {",
        "char *s = \"double\";",
        "char c = 'x'; char q = '\\'';",
        "char *e = \"esc: \\\" ok\";",
        "/* block */ static int y;",
        "/* multi\nline */ int z = 2;",
        "#include <iostream>",
        "#define MAX 42",
        "class Foo : public Bar { virtual ~Foo(); };",
        "namespace ns { using std::string; }",
        "template <typename T> T max(T a, T b) { return a > b ? a : b; }",
        "auto p = new Foo(); delete p; if (p == nullptr) return;",
        "for (const auto &item : items) continue;",
        "\"\" ''",
        "classclass class_ _class 1class",
        "int size = sizeof(Foo);",
        "\nint at_line_start\nreturn at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
