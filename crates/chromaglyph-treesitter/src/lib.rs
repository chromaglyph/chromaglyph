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

/// Extract semantic spans for a language (`"rust"`, `"js"`/`"ts"`,
/// `"python"` and their aliases); unknown languages yield nothing (the
/// syntactic colors stay).
pub fn semantic_spans(lang: &str, text: &str) -> Vec<SemanticSpan> {
    match lang.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => parse_with(
            tree_sitter_rust::LANGUAGE.into(),
            text,
            handle_rust,
        ),
        "js" | "javascript" | "jsx" | "mjs" | "cjs" => parse_with(
            tree_sitter_javascript::LANGUAGE.into(),
            text,
            handle_js,
        ),
        "ts" | "typescript" | "tsx" => parse_with(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            text,
            handle_js,
        ),
        "py" | "python" => parse_with(
            tree_sitter_python::LANGUAGE.into(),
            text,
            handle_python,
        ),
        _ => Vec::new(),
    }
}

type Handler = fn(Node, &[u8], &mut Vec<SemanticSpan>);

fn parse_with(language: tree_sitter::Language, text: &str, handle: Handler) -> Vec<SemanticSpan> {
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let source = text.as_bytes();
    let mut spans = Vec::new();
    walk(&mut tree.walk(), source, handle, &mut spans);
    spans
}

fn walk(cursor: &mut TreeCursor, source: &[u8], handle: Handler, spans: &mut Vec<SemanticSpan>) {
    loop {
        handle(cursor.node(), source, spans);
        if cursor.goto_first_child() {
            loop {
                walk(cursor, source, handle, spans);
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

fn handle_rust(node: Node, _source: &[u8], spans: &mut Vec<SemanticSpan>) {
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

/// JavaScript/TypeScript handler. Node kinds shared by both grammars
/// (typescript is a superset), so one handler serves both.
fn handle_js(node: Node, _source: &[u8], spans: &mut Vec<SemanticSpan>) {
    match node.kind() {
        "function_declaration" | "method_definition" | "class_declaration" => {
            let scope = if node.kind() == "class_declaration" { TYPE } else { FUNCTION };
            push_field(node, "name", scope, spans);
        }
        // `const f = () => {}` / `const f = function () {}`: the binding
        // names a function; `const N = <literal>` names a constant
        "variable_declarator" => {
            let value_is_fn = node
                .child_by_field_name("value")
                .is_some_and(|v| matches!(v.kind(), "arrow_function" | "function_expression" | "function"));
            if value_is_fn {
                push_field(node, "name", FUNCTION, spans);
            } else if let Some(name) = node.child_by_field_name("name") {
                let is_const_literal = node
                    .parent()
                    .is_some_and(|p| p.kind() == "lexical_declaration")
                    && node.child_by_field_name("value").is_some_and(|v| {
                        matches!(v.kind(), "number" | "string" | "template_literal" | "true" | "false" | "null")
                    });
                if is_const_literal {
                    push_range(name, CONSTANT, spans);
                }
            }
        }
        "formal_parameters" => {
            // bare identifiers, or the identifier inside TS
            // required/optional parameter wrappers
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    match c.kind() {
                        "identifier" => push_range(c, PARAMETER, spans),
                        "required_parameter" | "optional_parameter" => {
                            if let Some(id) = c.child_by_field_name("pattern").or_else(|| c.child(0)) {
                                if id.kind() == "identifier" {
                                    push_range(id, PARAMETER, spans);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        "call_expression" => {
            let Some(func) = node.child_by_field_name("function") else {
                return;
            };
            match func.kind() {
                "identifier" => push_range(func, FUNCTION, spans),
                "member_expression" => push_field(func, "property", FUNCTION, spans),
                _ => {}
            }
        }
        // property access `obj.x` (but not the method position of a call,
        // which `call_expression` above already colors as a function)
        "member_expression" => {
            let is_call_target = node
                .parent()
                .is_some_and(|p| p.kind() == "call_expression" && p.child_by_field_name("function") == Some(node));
            if !is_call_target {
                push_field(node, "property", PROPERTY, spans);
            }
        }
        "new_expression" => {
            let Some(ctor) = node.child_by_field_name("constructor") else {
                return;
            };
            if ctor.kind() == "identifier" {
                push_range(ctor, TYPE, spans);
            }
        }
        // typescript annotations: declared type names and the builtins
        // (string, number, …) which the grammar types as predefined_type
        "type_identifier" | "predefined_type" => push_range(node, TYPE, spans),
        _ => {}
    }
}

/// Python handler.
fn handle_python(node: Node, source: &[u8], spans: &mut Vec<SemanticSpan>) {
    match node.kind() {
        "function_definition" => push_field(node, "name", FUNCTION, spans),
        "class_definition" => push_field(node, "name", TYPE, spans),
        // parameters: bare identifiers and the identifier inside
        // typed/default parameters
        "parameters" | "typed_parameter" | "default_parameter" => {
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    if c.kind() == "identifier" {
                        push_range(c, PARAMETER, spans);
                    }
                }
            }
        }
        // the python grammar names its call node `call`
        "call" => {
            let Some(func) = node.child_by_field_name("function") else {
                return;
            };
            match func.kind() {
                "identifier" => push_range(func, FUNCTION, spans),
                "attribute" => push_field(func, "attribute", FUNCTION, spans),
                _ => {}
            }
        }
        // attribute access `obj.x` (not the method position of a call)
        "attribute" => {
            let is_call_target = node
                .parent()
                .is_some_and(|p| p.kind() == "call" && p.child_by_field_name("function") == Some(node));
            if !is_call_target {
                push_field(node, "attribute", PROPERTY, spans);
            }
        }
        // UPPER_CASE assignment targets read as constants (how editors
        // treat them; themes that color the scope opt in)
        "assignment" => {
            let Some(target) = node.child_by_field_name("left") else {
                return;
            };
            if target.kind() == "identifier" {
                if let Ok(name) = target.utf8_text(source) {
                    if is_upper_const(name) {
                        push_range(target, CONSTANT, spans);
                    }
                }
            }
        }
        // type annotations
        "type_identifier" => push_range(node, TYPE, spans),
        _ => {}
    }
}

fn is_upper_const(name: &str) -> bool {
    !name.is_empty()
        && name.chars().any(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
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
        let src = "const MAX: u32 = 9;\nlet v: Vec<i32> = Vec::new();\nv.iter().map(|n| n).count();\n";
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
    fn js_functions_calls_properties() {
        let src = "function add(a, b) { return a + b }\nconst f = (x) => x;\nconst N = 42;\nlet r = add(1, 2);\nlet v = obj.item;\nobj.run();\nclass Foo {}\nlet z = new Foo();\n";
        let spans = semantic_spans("js", src);
        let all = |needle: &str| {
            spans
                .iter()
                .filter(|s| &src[s.start..s.end] == needle)
                .map(|s| s.scope)
                .collect::<Vec<_>>()
        };
        assert!(all("add").contains(&FUNCTION), "def + call");
        assert!(all("a").contains(&PARAMETER));
        assert!(all("x").contains(&PARAMETER), "arrow parameter");
        assert!(all("f").contains(&FUNCTION), "const f = () => ..");
        assert!(all("N").contains(&CONSTANT), "const N = 42");
        assert!(all("item").contains(&PROPERTY), "obj.item");
        assert!(all("run").contains(&FUNCTION), "obj.run() colors the method");
        assert!(all("Foo").contains(&TYPE), "class + new");
    }

    #[test]
    fn ts_types_and_methods() {
        let src = "function size(v: string): number { return v.length }\n";
        let spans = semantic_spans("ts", src);
        let has = |needle: &str, scope: &str| {
            spans
                .iter()
                .any(|s| &src[s.start..s.end] == needle && s.scope == scope)
        };
        assert!(has("size", FUNCTION));
        assert!(has("v", PARAMETER));
        assert!(has("string", TYPE), "annotation type");
        assert!(has("number", TYPE), "return type");
        assert!(has("length", PROPERTY));
    }

    #[test]
    fn python_defs_calls_attributes() {
        let src = "def add(a, b):\n    return a + b\nMAX = 100\ntotal = add(1, 2)\nprint(obj.item)\nobj.run()\nclass Foo: pass\n";
        let spans = semantic_spans("python", src);
        let all = |needle: &str| {
            spans
                .iter()
                .filter(|s| &src[s.start..s.end] == needle)
                .map(|s| s.scope)
                .collect::<Vec<_>>()
        };
        assert!(all("add").contains(&FUNCTION), "def + call");
        assert!(all("a").contains(&PARAMETER));
        assert!(all("MAX").contains(&CONSTANT), "UPPER_CASE assignment");
        assert!(all("item").contains(&PROPERTY));
        assert!(all("run").contains(&FUNCTION), "obj.run() colors the method");
        assert!(all("Foo").contains(&TYPE));
        assert!(all("print").contains(&FUNCTION));
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
