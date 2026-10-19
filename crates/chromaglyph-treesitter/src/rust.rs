//! Rust: definitions, calls, field access and variable reads.

use crate::{
    push_field, push_range, SemanticSpan, CONSTANT, ENUM_MEMBER, FUNCTION, PARAMETER, PROPERTY,
    TYPE, VARIABLE,
};
use tree_sitter::Node;

pub(crate) fn handle_rust(node: Node, _source: &[u8], spans: &mut Vec<SemanticSpan>) {
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
            let is_call_target = node.parent().is_some_and(|p| {
                p.kind() == "call_expression" && p.child_by_field_name("function") == Some(node)
            });
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
        // bare identifiers are variable reads — what an editor's semantic
        // layer colors. Excluded: every position a branch above (or below
        // in dispatch order) already scopes — call/macro names, field
        // access, path segments, declaration names, parameters. Parents
        // are handled before children, so an unexcluded identifier here
        // would otherwise overwrite those spans
        "identifier" => {
            let already_scoped = node.parent().is_some_and(|p| {
                let name_pos = p.child_by_field_name("name") == Some(node);
                match p.kind() {
                    "call_expression" | "macro_invocation" | "scoped_identifier"
                    | "closure_parameters" | "parameters" | "enum_variant"
                    | "field_declaration" => true,
                    // only the field position: the receiver of `v.iter()`
                    // is itself a variable read
                    "field_expression" => p.child_by_field_name("field") == Some(node),
                    "function_item" | "const_item" | "static_item" | "struct_item"
                    | "enum_item" | "trait_item" | "mod_item" | "macro_definition" => name_pos,
                    _ => false,
                }
            });
            if !already_scoped {
                push_range(node, VARIABLE, spans);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_spans;

    #[test]
    fn functions_parameters_and_calls() {
        let src = "fn add(a: i32, b: i32) -> i32 { a + b }\nlet s = add(1, 2);\n";
        let spans = semantic_spans("rust", src);
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
        let src =
            "const MAX: u32 = 9;\nlet v: Vec<i32> = Vec::new();\nv.iter().map(|n| n).count();\n";
        let spans = semantic_spans("rust", src);
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
    fn rust_variables_read_as_semantic_blue() {
        let src = "fn main() {\n    let path = \"src\";\n    let msg = format!(\"{}\", path);\n    println!(\"{}\", msg);\n}\n";
        let spans = semantic_spans("rust", src);
        let get = |needle: &str| {
            spans
                .iter()
                .filter(|s| &src[s.start..s.end] == needle)
                .map(|s| s.scope)
                .collect::<Vec<_>>()
        };
        // variables get the semantic variable scope
        assert!(get("path").contains(&VARIABLE));
        assert!(get("msg").contains(&VARIABLE));
        // but names a branch already scopes keep their color
        assert!(get("format").contains(&FUNCTION));
        assert!(get("println").contains(&FUNCTION));
        assert!(get("main").contains(&FUNCTION));
    }

    #[test]
    fn enum_members_and_fields() {
        let src = "enum E { A, B }\nstruct P { x: i32 }\nlet q = E::A;\nlet n = P { x: 1 }.x;\n";
        let spans = semantic_spans("rust", src);
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
