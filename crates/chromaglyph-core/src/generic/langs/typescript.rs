//! Built-in TypeScript spec: JavaScript plus its type/declaration words.  (constructor + curated oracle samples)

use crate::generic::{KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "typescript",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.ts",
                words: &[
                    "as",
                    "await",
                    "break",
                    "case",
                    "catch",
                    "continue",
                    "default",
                    "delete",
                    "do",
                    "else",
                    "export",
                    "finally",
                    "for",
                    "from",
                    "if",
                    "import",
                    "in",
                    "instanceof",
                    "new",
                    "return",
                    "satisfies",
                    "switch",
                    "throw",
                    "try",
                    "typeof",
                    "while",
                    "with",
                    "debugger",
                    "yield",
                ],
            },
            KeywordGroup {
                scope: "storage.type.ts",
                words: &[
                    "var",
                    "let",
                    "const",
                    "function",
                    "class",
                    "extends",
                    "super",
                    "static",
                    "get",
                    "set",
                    "async",
                    "of",
                    "interface",
                    "type",
                    "enum",
                    "implements",
                    "private",
                    "public",
                    "protected",
                    "readonly",
                    "namespace",
                    "declare",
                    "abstract",
                    "keyof",
                    "infer",
                    "is",
                    "asserts",
                    "override",
                    "module",
                    "any",
                    "boolean",
                    "number",
                    "string",
                    "symbol",
                    "object",
                    "never",
                    "unknown",
                    "bigint",
                    "void",
                ],
            },
            KeywordGroup {
                scope: "constant.language.ts",
                words: &["true", "false", "null", "undefined", "this"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.double.ts",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.ts",
            },
            StringKind {
                quote: '`',
                escapes: true,
                interpolation: Some(("${", '}')),
                scope: "string.template.ts",
            },
        ],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.ts",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.block.ts",
            },
        ],
        numbers: true,
        number_scope: "constant.numeric.ts",
        ident_extra: &['$'],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, keyword-boundary soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "interface I { x: number; }",
        "type T = Array<string>;",
        "const x: Array<number> = [1, 2];",
        "enum E { A, B }",
        "abstract class C implements I { private readonly x?: string; }",
        "function f<T extends object>(v: T): T { return v; }",
        "declare module 'm' { export const x: 1; }",
        "const y = x as unknown as string;",
        "const z = w satisfies T;",
        "let a: any = null; let b: void; let c: unknown;",
        "type K = keyof O;",
        "is x string? asserts v is T: infer U;",
        "// comment\nconst s: string = `t ${x}`;",
        "/* multi\nline */ let t: boolean;",
        "x = 42; y = 3.14; z = 0xFF;",
        "namespace N { export function g(): never { throw 1; } }",
        "class D extends C implements I { override m() {} }",
        "\ninterface at_line_start\nconst at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
