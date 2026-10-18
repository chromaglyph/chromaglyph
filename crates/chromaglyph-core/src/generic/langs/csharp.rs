//! Built-in C# spec: `"` strings, `'` char literals, `@"` verbatim
//! strings (as a span — the engine's span machinery handles the
//! multi-char opener, and the doubled-quote escape of real verbatim
//! strings is approximated by closing at the first quote), `//`, `///`
//! and `/* */` comments.

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "csharp",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.cs",
                followed_by: None,
                value: false,
                words: &[
                    "break",
                    "case",
                    "catch",
                    "checked",
                    "continue",
                    "do",
                    "else",
                    "finally",
                    "fixed",
                    "for",
                    "foreach",
                    "goto",
                    "if",
                    "in",
                    "lock",
                    "return",
                    "switch",
                    "throw",
                    "try",
                    "unchecked",
                    "unsafe",
                    "while",
                    "yield",
                ],
            },
            KeywordGroup {
                scope: "storage.type.cs",
                followed_by: None,
                value: false,
                words: &[
                    "bool",
                    "byte",
                    "char",
                    "class",
                    "decimal",
                    "delegate",
                    "double",
                    "dynamic",
                    "enum",
                    "float",
                    "int",
                    "interface",
                    "long",
                    "object",
                    "record",
                    "sbyte",
                    "short",
                    "string",
                    "struct",
                    "uint",
                    "ulong",
                    "ushort",
                    "var",
                    "void",
                ],
            },
            KeywordGroup {
                scope: "storage.modifier.cs",
                followed_by: None,
                value: false,
                words: &[
                    "abstract",
                    "async",
                    "const",
                    "event",
                    "extern",
                    "internal",
                    "new",
                    "out",
                    "override",
                    "params",
                    "private",
                    "protected",
                    "public",
                    "readonly",
                    "ref",
                    "sealed",
                    "static",
                    "virtual",
                    "volatile",
                ],
            },
            KeywordGroup {
                scope: "keyword.other.cs",
                followed_by: None,
                value: false,
                words: &[
                    "as",
                    "await",
                    "base",
                    "default",
                    "explicit",
                    "implicit",
                    "is",
                    "namespace",
                    "nameof",
                    "operator",
                    "sizeof",
                    "stackalloc",
                    "this",
                    "typeof",
                    "using",
                ],
            },
            KeywordGroup {
                scope: "constant.language.cs",
                followed_by: None,
                value: false,
                words: &["true", "false", "null"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                // $"interpolated": the `$` prefix stays plain (it is not
                // an identifier char) and the quote opens an ordinary
                // string — `{}` holes render as string body, which is the
                // line-scoped best effort for a flat engine
                interpolation: None,
                scope: "string.quoted.double.cs",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.cs",
            },
        ],
        spans: &[
            SpanKind {
                open: "///",
                close: "",
                scope: "comment.documentation.cs",
            },
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.cs",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.block.cs",
            },
            // verbatim string: no escapes, a doubled `""` is a literal
            // quote — approximated by closing at the first one
            SpanKind {
                open: "@\"",
                close: "\"",
                scope: "string.quoted.double.verbatim.cs",
            },
        ],
        tags: &[],
        blocks: &[],
        numbers: true,
        number_scope: "constant.numeric.cs",
        // '@' verbatim identifiers (@return) must not color the word after
        // them — '@' lives in a span opener, so it is not a boundary glyph
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
        "string s = \"double\";",
        "char c = 'x'; char q = '\\'';",
        "string e = \"esc: \\\" ok\";",
        "string v = @\"verbatim \\n stays\";",
        "string i = $\"hello {name}!\";",
        "/* block */ private int y;",
        "/* multi\nline */ int z = 2;",
        "var list = new List<int>();",
        "foreach (var item in list) continue;",
        "if (x is string s) return null;",
        "using var db = new DbContext();",
        "await Task.Run(() => nameof(A));",
        "\"\" '' @\"\" $\"\"",
        "classclass class_ _class 1class",
        "@return return @return",
        "switch (x) { case 1: break; }",
        "/// doc comment\npublic static void Main() {}",
        "\nclass at_line_start\nint at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
