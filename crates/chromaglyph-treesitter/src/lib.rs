//! Optional tree-sitter semantic layer for chromaglyph.
//!
//! The core pipeline is syntactic (syntect TextMate grammars). This crate
//! plugs into core's `semantic` hook: it parses code with tree-sitter and
//! turns the syntax tree into semantic spans — `(byte range, dotted
//! TextMate scope)` triples like `entity.name.function` for a function
//! call or `variable.parameter` for a parameter — which core overlays on
//! the syntactic colors wherever the theme colors the scope. The result
//! looks like an editor with a language server: parameters, types,
//! constants and calls get their semantic colors on top of keywords,
//! strings and comments.
//!
//! This is the only crate in the workspace with C dependencies (the
//! tree-sitter runtime and grammars); `chromaglyph-core` stays pure Rust.

use tree_sitter::{Node, Parser, TreeCursor};

/// One semantic span: a byte range in the source plus the TextMate-style
/// dotted scope a theme can color.
#[derive(Clone, Copy, Debug)]
pub struct SemanticSpan {
    pub start: usize,
    pub end: usize,
    pub scope: &'static str,
}

const FUNCTION: &str = "entity.name.function";
const PARAMETER: &str = "variable.parameter";
const TYPE: &str = "entity.name.type";
const CONSTANT: &str = "constant.other";
const ENUM_MEMBER: &str = "entity.name.enum.constant";
const PROPERTY: &str = "variable.other.property";

/// Extract semantic spans for a language (`"rust"`, `"rs"`); unknown
/// languages yield nothing (the syntactic colors stay).
pub fn semantic_spans(lang: &str, text: &str) -> Vec<SemanticSpan> {
    match lang.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => rust_spans(text),
        _ => Vec::new(),
    }
}

fn rust_spans(text: &str) -> Vec<SemanticSpan> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    walk(&mut tree.walk(), &mut spans);
    spans
}

fn walk(cursor: &mut TreeCursor, spans: &mut Vec<SemanticSpan>) {
    loop {
        handle(cursor.node(), spans);
        if cursor.goto_first_child() {
            loop {
                walk(cursor, spans);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn handle(node: Node, spans: &mut Vec<SemanticSpan>) {
    match node.kind() {
        // fn definitions: the name
        "function_item" => push_field(node, "name", FUNCTION, spans),
        // const/static items: the name
        "const_item" | "static_item" => push_field(node, "name", CONSTANT, spans),
        // enum variants
        "enum_variant" => push_field(node, "name", ENUM_MEMBER, spans),
        // struct/union field declarations
        "field_declaration" => push_field(node, "name", PROPERTY, spans),
        // function parameters
        "parameter" => push_field(node, "pattern", PARAMETER, spans),
        // closure parameters are bare identifiers
        "closure_parameters" => {
            let mut child = node.walk();
            for i in 0..node.child_count() {
                if node.child(i).is_some_and(|c| c.kind() == "identifier") {
                    if let Some(c) = node.child(i) {
                        push_range(c, PARAMETER, spans);
                    }
                }
            }
            let _ = &mut child;
        }
        // field access `p.x` (but not the method-name position of a call,
        // which `call_expression` below already colors as a function)
        "field_expression" => {
            let is_call_target = node
                .parent()
                .is_some_and(|p| p.kind() == "call_expression" && p.child_by_field_name("function") == Some(node));
            if !is_call_target {
                push_field(node, "field", PROPERTY, spans);
            }
        }
        // calls: `add(..)`, `Vec::new(..)`, `v.iter().map(..)`
        "call_expression" => {
            let Some(func) = node.child_by_field_name("function") else {
                return;
            };
            match func.kind() {
                "identifier" => push_range(func, FUNCTION, spans),
                "scoped_identifier" => {
                    push_field(func, "name", FUNCTION, spans);
                    push_field(func, "path", TYPE, spans);
                }
                "field_expression" => push_field(func, "field", FUNCTION, spans),
                _ => {}
            }
        }
        // macro invocations: the macro name
        "macro_invocation" => push_field(node, "macro", FUNCTION, spans),
        // type usages: `i32`, `Vec`, declared type names
        "type_identifier" => push_range(node, TYPE, spans),
        _ => {}
    }
}

fn push_range(node: Node, scope: &'static str, spans: &mut Vec<SemanticSpan>) {
    spans.push(SemanticSpan {
        start: node.start_byte(),
        end: node.end_byte(),
        scope,
    });
}

fn push_field(node: Node, field: &str, scope: &'static str, spans: &mut Vec<SemanticSpan>) {
    if let Some(child) = node.child_by_field_name(field) {
        push_range(child, scope, spans);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn functions_parameters_and_calls() {
        let src = "fn add(a: i32, b: i32) -> i32 { a + b }\nlet s = add(1, 2);\n";
        let spans = rust_spans(src);
        let get = |needle: &str| {
            spans
                .iter()
                .find(|s| &src[s.start..s.end] == needle)
                .map(|s| s.scope)
        };
        assert_eq!(get("add"), Some(FUNCTION)); // def name; the call also spans FUNCTION
        assert_eq!(get("a"), Some(PARAMETER));
        assert_eq!(get("b"), Some(PARAMETER));
        // primitive types are  nodes and stay syntactic
    }

    #[test]
    fn methods_types_and_constants() {
        let src = "const MAX: u32 = 9;\nlet v: Vec<i32> = Vec::new();\nv.iter().map(|n| n).count();\n";
        let spans = rust_spans(src);
        let get = |needle: &str| {
            spans
                .iter()
                .filter(|s| &src[s.start..s.end] == needle)
                .map(|s| s.scope)
                .collect::<Vec<_>>()
        };
        assert!(get("MAX").contains(&CONSTANT));
        assert!(get("Vec").contains(&TYPE));
        assert!(get("new").contains(&FUNCTION));
        assert!(get("iter").contains(&FUNCTION)); // method, not property
        assert!(get("map").contains(&FUNCTION));
        assert!(get("n").contains(&PARAMETER));
    }

    #[test]
    fn enum_members_and_fields() {
        let src = "enum E { A, B }\nstruct P { x: i32 }\nlet q = E::A;\nlet n = P { x: 1 }.x;\n";
        let spans = rust_spans(src);
        let get = |needle: &str| {
            spans
                .iter()
                .any(|s| &src[s.start..s.end] == needle && s.scope == ENUM_MEMBER)
        };
        assert!(get("A") && get("B"));
        assert!(spans
            .iter()
            .any(|s| &src[s.start..s.end] == "x" && s.scope == PROPERTY));
    }
}
