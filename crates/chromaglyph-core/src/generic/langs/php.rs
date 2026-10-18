//! Built-in PHP spec: `$` variables (an identifier char, so `$return`
//! is a name), `"` strings with `{$...}` interpolation, `'` strings,
//! `//` `#` `/* */` comments, numbers. The `<?php` tag and attributes
//! are not modeled — coloring is uniform, the tag renders like any
//! other text.

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "php",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.php",
                followed_by: None,
                value: false,
                words: &[
                    "break", "case", "continue", "default", "do", "else", "elseif", "for",
                    "foreach", "goto", "if", "return", "switch", "throw", "try", "while", "yield",
                ],
            },
            KeywordGroup {
                scope: "storage.type.php",
                followed_by: None,
                value: false,
                words: &[
                    "array",
                    "bool",
                    "callable",
                    "class",
                    "const",
                    "enum",
                    "float",
                    "fn",
                    "function",
                    "global",
                    "int",
                    "interface",
                    "iterable",
                    "list",
                    "namespace",
                    "object",
                    "readonly",
                    "string",
                    "trait",
                    "void",
                ],
            },
            KeywordGroup {
                scope: "storage.modifier.php",
                followed_by: None,
                value: false,
                words: &[
                    "abstract",
                    "final",
                    "private",
                    "protected",
                    "public",
                    "static",
                ],
            },
            KeywordGroup {
                scope: "keyword.other.php",
                followed_by: None,
                value: false,
                words: &[
                    "as",
                    "clone",
                    "echo",
                    "extends",
                    "implements",
                    "include",
                    "include_once",
                    "instanceof",
                    "insteadof",
                    "new",
                    "print",
                    "require",
                    "require_once",
                    "unset",
                    "use",
                    "var",
                ],
            },
            KeywordGroup {
                scope: "constant.language.php",
                followed_by: None,
                value: false,
                words: &["true", "false", "null"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: Some(("{$", '}')),
                scope: "string.quoted.double.php",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.php",
            },
        ],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.php",
            },
            SpanKind {
                open: "#",
                close: "",
                scope: "comment.line.number-sign.php",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.block.php",
            },
        ],
        tags: &[],
        blocks: &[],
        numbers: true,
        number_scope: "constant.numeric.php",
        ident_extra: &['$'],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "$x = 1;",
        "// comment\n# also\npublic function f()",
        "$s = \"double {$x} inside\";",
        "$c = 'single';",
        "$e = \"esc: \\\" ok\";",
        "'it\\'s' \"a\\\"b\"",
        "\"$plain stays string body\";",
        "foreach ($items as $item) { echo $item; }",
        "class Foo extends Bar implements Baz {}",
        "if ($x instanceof Foo) return null;",
        "function f(int $n): string { return \"$n\"; }",
        "\"\" '' $\"\"",
        "functionfunction function_ _function 1function",
        "$function function$ $function$",
        "if ($x)\n    $y = 2;",
        "\nfunction at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
