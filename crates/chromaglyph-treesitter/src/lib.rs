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
const VARIABLE: &str = "variable.other.readwrite";

/// Extract semantic spans for a language (`"rust"`, `"js"`/`"ts"`,
/// `"python"` and their aliases); unknown languages yield nothing (the
/// syntactic colors stay).
pub fn semantic_spans(lang: &str, text: &str) -> Vec<SemanticSpan> {
    match lang.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => parse_with(tree_sitter_rust::LANGUAGE.into(), text, handle_rust),
        "js" | "javascript" | "jsx" | "mjs" | "cjs" => {
            parse_with(tree_sitter_javascript::LANGUAGE.into(), text, handle_js)
        }
        "ts" | "typescript" | "tsx" => parse_with(
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            text,
            handle_js,
        ),
        "py" | "python" => parse_with(tree_sitter_python::LANGUAGE.into(), text, handle_python),
        "java" => parse_with(tree_sitter_java::LANGUAGE.into(), text, handle_java),
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

/// JavaScript/TypeScript handler. Node kinds shared by both grammars
/// (typescript is a superset), so one handler serves both.
fn handle_js(node: Node, _source: &[u8], spans: &mut Vec<SemanticSpan>) {
    match node.kind() {
        "function_declaration" | "method_definition" | "class_declaration" => {
            let scope = if node.kind() == "class_declaration" {
                TYPE
            } else {
                FUNCTION
            };
            push_field(node, "name", scope, spans);
        }
        // `const f = () => {}` / `const f = function () {}`: the binding
        // names a function; `const N = <literal>` names a constant
        "variable_declarator" => {
            let value_is_fn = node.child_by_field_name("value").is_some_and(|v| {
                matches!(
                    v.kind(),
                    "arrow_function" | "function_expression" | "function"
                )
            });
            if value_is_fn {
                push_field(node, "name", FUNCTION, spans);
            } else if let Some(name) = node.child_by_field_name("name") {
                let is_const_literal = node
                    .parent()
                    .is_some_and(|p| p.kind() == "lexical_declaration")
                    && node.child_by_field_name("value").is_some_and(|v| {
                        matches!(
                            v.kind(),
                            "number" | "string" | "template_literal" | "true" | "false" | "null"
                        )
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
                            if let Some(id) =
                                c.child_by_field_name("pattern").or_else(|| c.child(0))
                            {
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
            let is_call_target = node.parent().is_some_and(|p| {
                p.kind() == "call_expression" && p.child_by_field_name("function") == Some(node)
            });
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
        "class_definition" => {
            push_field(node, "name", TYPE, spans);
            // base classes in the superclass list read as types (how
            // editors color them); keyword args there stay syntactic
            if let Some(supers) = node.child_by_field_name("superclasses") {
                for i in 0..supers.child_count() {
                    if let Some(c) = supers.child(i) {
                        if c.kind() == "identifier" {
                            push_range(c, TYPE, spans);
                        }
                    }
                }
            }
        }
        // parameters: bare identifiers, the identifier inside
        // typed/default parameters, and *args/**kw splats
        "parameters"
        | "typed_parameter"
        | "default_parameter"
        | "typed_default_parameter"
        | "list_splat_pattern"
        | "dictionary_splat_pattern" => {
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
            let is_call_target = node.parent().is_some_and(|p| {
                p.kind() == "call" && p.child_by_field_name("function") == Some(node)
            });
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
        // bare identifiers are variable reads/writes — the semantic blue.
        // Excluded: every position a branch above (or below in dispatch
        // order) already scopes — call/attribute names, parameters,
        // definition names, UPPER_CASE assignment targets, import paths.
        // Parents are handled before children, so an unexcluded identifier
        // here would overwrite those spans. `attribute` excludes only the
        // attribute position: the object of `obj.run()` is itself a
        // variable read and keeps this color
        "identifier" => {
            let already_scoped = node.parent().is_some_and(|p| match p.kind() {
                "call" => p.child_by_field_name("function") == Some(node),
                "attribute" => p.child_by_field_name("attribute") == Some(node),
                "keyword_argument" => p.child_by_field_name("name") == Some(node),
                "assignment" => p.child_by_field_name("left") == Some(node),
                "function_definition" | "class_definition" => {
                    p.child_by_field_name("name") == Some(node)
                }
                "parameters"
                | "typed_parameter"
                | "default_parameter"
                | "typed_default_parameter"
                | "list_splat_pattern"
                | "dictionary_splat_pattern"
                | "dotted_name"
                | "aliased_import"
                | "decorator" => true,
                // the base-class list: TYPE is pushed for identifiers there
                // by the class_definition branch. Any other argument_list
                // (call arguments) is made of variable reads — keep those
                "argument_list" => p.parent().is_some_and(|g| g.kind() == "class_definition"),
                _ => false,
            });
            if !already_scoped {
                push_range(node, VARIABLE, spans);
            }
        }
        _ => {}
    }
}

/// Java handler.
fn handle_java(node: Node, _source: &[u8], spans: &mut Vec<SemanticSpan>) {
    match node.kind() {
        "method_declaration" | "constructor_declaration" => {
            push_field(node, "name", FUNCTION, spans);
        }
        "class_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "record_declaration"
        | "annotation_type_declaration" => {
            push_field(node, "name", TYPE, spans);
        }
        "enum_constant" => push_field(node, "name", ENUM_MEMBER, spans),
        // lambda parameters are bare identifiers directly under
        // formal_parameters; method/constructor params are formal_parameter
        // nodes (own branch below)
        "formal_parameters" => {
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    if c.kind() == "identifier" {
                        push_range(c, PARAMETER, spans);
                    }
                }
            }
        }
        "formal_parameter" => push_field(node, "name", PARAMETER, spans),
        // a single lambda parameter is a bare identifier in the
        // `parameters` field (`x -> x * x`), not inside formal_parameters
        "lambda_expression" => {
            if let Some(p) = node.child_by_field_name("parameters") {
                if p.kind() == "identifier" {
                    push_range(p, PARAMETER, spans);
                }
            }
        }
        // `String... rest` wraps the name in a declarator
        "spread_parameter" => {
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    if c.kind() == "variable_declarator" {
                        push_field(c, "name", PARAMETER, spans);
                    }
                }
            }
        }
        "method_invocation" => push_field(node, "name", FUNCTION, spans),
        // field access `obj.x`, `this.x`. Unlike rust/js/python, Java's
        // call target is always the `name` field of method_invocation,
        // never the receiver expression — no call-target exclusion needed
        "field_access" => push_field(node, "field", PROPERTY, spans),
        "object_creation_expression" => push_field(node, "type", TYPE, spans),
        // `Map.Entry`: the scope segment reads as a type too
        "scoped_type_identifier" => {
            if let Some(scope) = node.child_by_field_name("scope") {
                if scope.kind() == "identifier" {
                    push_range(scope, TYPE, spans);
                }
            }
        }
        // declaration sites: a lambda/method-ref binding names a function,
        // a `final` field with a literal value is a constant, other fields
        // are properties, locals (and for/lambda bindings) are variables
        "variable_declarator" => {
            let Some(name) = node.child_by_field_name("name") else {
                return;
            };
            let value = node.child_by_field_name("value");
            if value.is_some_and(|v| matches!(v.kind(), "lambda_expression" | "method_reference")) {
                push_range(name, FUNCTION, spans);
                return;
            }
            let scope = match node.parent().map(|p| p.kind()) {
                Some("field_declaration") => {
                    let is_final_field = node.parent().is_some_and(|p| has_final_modifier(p))
                        && value.is_some_and(is_literal);
                    if is_final_field {
                        CONSTANT
                    } else {
                        PROPERTY
                    }
                }
                _ => VARIABLE,
            };
            push_range(name, scope, spans);
        }
        "type_identifier" => push_range(node, TYPE, spans),
        // bare identifiers are variable reads/writes — the semantic blue.
        // Excluded: every position a branch above (or below in dispatch
        // order) already scopes — method/declaration/declarator names,
        // parameters, field positions, import/package paths (`Map` in
        // Map.Entry gets TYPE from scoped_type_identifier). The receiver
        // of `obj.run()` is a variable read and keeps this color
        "identifier" => {
            let already_scoped = node.parent().is_some_and(|p| {
                let name_pos = p.child_by_field_name("name") == Some(node);
                match p.kind() {
                    "method_invocation"
                    | "formal_parameter"
                    | "variable_declarator"
                    | "marker_annotation"
                    | "enum_constant" => name_pos,
                    "lambda_expression" => p.child_by_field_name("parameters") == Some(node),
                    "field_access" => p.child_by_field_name("field") == Some(node),
                    "scoped_identifier"
                    | "scoped_type_identifier"
                    | "method_reference"
                    | "labeled_statement" => true,
                    "method_declaration"
                    | "constructor_declaration"
                    | "class_declaration"
                    | "interface_declaration"
                    | "enum_declaration"
                    | "record_declaration"
                    | "annotation_type_declaration" => name_pos,
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

/// Whether a declaration carries a `final` in its modifiers
/// (`private static final int MAX = 9`).
fn has_final_modifier(decl: Node) -> bool {
    (0..decl.child_count()).any(|i| {
        decl.child(i).is_some_and(|c| {
            c.kind() == "modifiers"
                && (0..c.child_count()).any(|j| c.child(j).is_some_and(|m| m.kind() == "final"))
        })
    })
}

/// Java literal expression kinds (for `final` field = constant).
fn is_literal(node: Node) -> bool {
    matches!(
        node.kind(),
        "decimal_integer_literal"
            | "hex_integer_literal"
            | "octal_integer_literal"
            | "binary_integer_literal"
            | "decimal_floating_point_literal"
            | "hex_floating_point_literal"
            | "string_literal"
            | "character_literal"
            | "true"
            | "false"
            | "null_literal"
    )
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
        assert!(
            all("run").contains(&FUNCTION),
            "obj.run() colors the method"
        );
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
        assert!(
            all("run").contains(&FUNCTION),
            "obj.run() colors the method"
        );
        assert!(all("Foo").contains(&TYPE));
        assert!(all("print").contains(&FUNCTION));
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

    #[test]
    fn python_variables_read_as_semantic_blue() {
        let src = "import os\n\
                    from mod import thing\n\
                    def f(a, b=1, c: int = 2, *args, **kw):\n\
                        total = a + b\n\
                        print(file=kw)\n\
                        for i in range(3):\n\
                            pass\n\
                        return obj.run(total)\n";
        let spans = semantic_spans("python", src);
        let all = |needle: &str| {
            spans
                .iter()
                .filter(|s| &src[s.start..s.end] == needle)
                .map(|s| s.scope)
                .collect::<Vec<_>>()
        };
        // reads and writes are variables
        assert!(all("total").contains(&VARIABLE));
        assert!(all("i").contains(&VARIABLE), "loop binding");
        assert!(all("obj").contains(&VARIABLE), "call receiver is a read");
        // parameters at the def site are parameters; reads inside the
        // body (`a + b`) are variables — both like an editor
        assert!(all("a").contains(&PARAMETER));
        assert!(all("a").contains(&VARIABLE), "the read in a + b");
        assert!(all("b").contains(&PARAMETER), "default value param");
        assert!(all("c").contains(&PARAMETER), "typed default param");
        assert!(!all("c").contains(&VARIABLE), "c is never read");
        assert!(all("args").contains(&PARAMETER), "*args");
        assert!(all("kw").contains(&PARAMETER), "**kw (the def site)");
        assert!(all("kw").contains(&VARIABLE), "**kw (the read in file=kw)");
        // keyword-argument labels, import paths and function names stay
        // in their own (syntactic) colors — no variable span at all
        assert!(all("file").is_empty(), "keyword argument label");
        assert!(all("os").is_empty() && all("thing").is_empty(), "imports");
        assert!(all("run").contains(&FUNCTION) && !all("run").contains(&VARIABLE));
        assert!(all("range").contains(&FUNCTION) && !all("range").contains(&VARIABLE));
    }

    #[test]
    fn python_upper_const_binding_is_not_overwritten() {
        // regression: the assignment branch pushes CONSTANT for the
        // UPPER_CASE target, but the bare-identifier branch used to walk
        // the same node afterwards and push VARIABLE over it (later spans
        // win in apply order)
        let src = "MAX = 100\nprint(MAX)\n";
        let spans = semantic_spans("python", src);
        let binding = spans
            .iter()
            .find(|s| &src[s.start..s.end] == "MAX")
            .expect("binding site has a span");
        let scope = binding.scope;
        assert_eq!(scope, CONSTANT);
        // exactly one span covers the binding site (no VARIABLE overlay)
        let overlapping: Vec<_> = spans
            .iter()
            .filter(|s| s.start == binding.start && s.end == binding.end)
            .collect();
        assert_eq!(overlapping.len(), 1);
    }

    #[test]
    fn python_superclasses_read_as_types() {
        let src = "class Foo(Base, metaclass=Meta):\n    pass\n";
        let spans = semantic_spans("python", src);
        let last = |needle: &str| {
            spans
                .iter()
                .filter(|s| &src[s.start..s.end] == needle)
                .map(|s| s.scope)
                .last()
        };
        assert_eq!(last("Foo"), Some(TYPE));
        // the TYPE span must be the last one covering the base — the
        // bare-identifier branch used to push VARIABLE over it
        assert_eq!(last("Base"), Some(TYPE), "base class is a type");
        // kwarg label stays syntactic; the Meta value position is a read
        assert!(
            spans
                .iter()
                .filter(|s| &src[s.start..s.end] == "metaclass")
                .count()
                == 0
        );
        assert_eq!(last("Meta"), Some(VARIABLE), "kwarg value is a read");
    }

    #[test]
    fn java_declarations_calls_and_variables() {
        let src = "package com.example.app;\n\
                    import java.util.List;\n\
                    public final class Point extends Base {\n\
                        private static final int MAX = 9;\n\
                        private int x;\n\
                        public Point(int x) { this.x = x; }\n\
                        public int getX() { return x; }\n\
                        Runnable task = () -> work();\n\
                        int twice(int v) { return v * 2; }\n\
                        int square(int q) {\n\
                            java.util.function.Function<Integer, Integer> f = w -> w * w;\n\
                            return f.apply(square(2));\n\
                        }\n\
                        void run(List<String> names, String... rest) {\n\
                            int total = 0;\n\
                            for (String n : names) { total += n.length(); }\n\
                            Point p = new Point(1);\n\
                            int v = p.x + MAX;\n\
                            String s = names.get(0);\n\
                        }\n\
                    }\n\
                    enum Color { RED, GREEN }\n\
                    @Override\n\
                    class Sub extends Point { }\n";
        let spans = semantic_spans("java", src);
        let all = |needle: &str| {
            spans
                .iter()
                .filter(|s| &src[s.start..s.end] == needle)
                .map(|s| s.scope)
                .collect::<Vec<_>>()
        };
        // declarations
        assert!(all("Point").contains(&TYPE), "class name");
        assert!(all("Point").contains(&FUNCTION), "constructor name");
        assert!(all("Base").contains(&TYPE), "superclass");
        assert!(all("getX").contains(&FUNCTION) && all("run").contains(&FUNCTION));
        assert!(all("Color").contains(&TYPE) && all("Sub").contains(&TYPE));
        assert!(all("RED").contains(&ENUM_MEMBER) && all("GREEN").contains(&ENUM_MEMBER));
        // fields and constants
        assert!(all("MAX").contains(&CONSTANT), "static final literal field");
        assert!(
            all("x").contains(&PROPERTY),
            "field decl + this.x assignment"
        );
        assert!(
            all("task").contains(&FUNCTION),
            "lambda binding names a function"
        );
        // parameters: typed, spread, lambda
        assert!(all("names").contains(&PARAMETER));
        assert!(all("rest").contains(&PARAMETER), "String... spread");
        assert!(all("w").contains(&PARAMETER), "single lambda param");
        assert!(all("v").contains(&PARAMETER) || all("v").contains(&VARIABLE));
        // variables: locals, loop bindings, receivers, reads
        assert!(all("total").contains(&VARIABLE));
        assert!(
            all("n").contains(&VARIABLE),
            "enhanced-for binding + receiver"
        );
        assert!(all("p").contains(&VARIABLE));
        assert!(all("f").contains(&VARIABLE), "local binding (not final)");
        // calls and accesses
        assert!(all("get").contains(&FUNCTION), "names.get(0)");
        assert!(all("apply").contains(&FUNCTION));
        assert!(all("work").contains(&FUNCTION));
        assert!(all("length").contains(&FUNCTION));
        assert!(all("square").contains(&FUNCTION), "def + recursive call");
        assert!(!all("names").contains(&FUNCTION));
        // p.x is a field access (property), p itself a variable
        let px = src.find("p.x").expect("sample has p.x");
        assert!(spans
            .iter()
            .any(|s| s.start == px && &src[s.start..s.end] == "p" && s.scope == VARIABLE));
        assert!(spans
            .iter()
            .any(|s| s.start == px + 2 && &src[s.start..s.end] == "x" && s.scope == PROPERTY));
        // imports, package and annotations carry no variable span
        assert!(all("com").is_empty() && all("example").is_empty());
        assert!(all("Override").is_empty());
        // `Point` in `new Point(1)` reads as a type
        let new_point = src.find("new Point").expect("sample has new Point");
        assert!(spans.iter().any(|s| s.start == new_point + 4
            && &src[s.start..s.end] == "Point"
            && s.scope == TYPE));
    }
}
