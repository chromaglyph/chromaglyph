//! JavaScript and TypeScript.

use crate::{push_field, push_range, SemanticSpan, CONSTANT, FUNCTION, PARAMETER, PROPERTY, TYPE};
use tree_sitter::Node;

/// JavaScript/TypeScript handler. Node kinds shared by both grammars
/// (typescript is a superset), so one handler serves both.
pub(crate) fn handle_js(node: Node, _source: &[u8], spans: &mut Vec<SemanticSpan>) {
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
                // `let` and `const` are both lexical_declaration in the
                // grammar; only a `const` with a literal value is a
                // constant (`let N = 42` is reassignable)
                let is_const_literal = node.parent().is_some_and(|p| {
                    p.kind() == "lexical_declaration"
                        && p.child(0).is_some_and(|kw| kw.kind() == "const")
                }) && node.child_by_field_name("value").is_some_and(|v| {
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
        // object-literal properties `{ msg: value }`: identifier keys color
        // like member access (`obj.msg`); quoted, numeric and computed keys
        // keep their syntactic colors (string, expression)
        "pair" => {
            if let Some(key) = node.child_by_field_name("key") {
                if key.kind() == "property_identifier" {
                    push_range(key, PROPERTY, spans);
                }
            }
        }
        // `{ msg }` shorthand in an expression. The destructuring twin
        // `shorthand_property_identifier_pattern` is a variable binding,
        // not a property, and stays syntactic
        "shorthand_property_identifier" => push_range(node, PROPERTY, spans),
        // typescript annotations: declared type names and the builtins
        // (string, number, …) which the grammar types as predefined_type
        "type_identifier" | "predefined_type" => push_range(node, TYPE, spans),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_spans;

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
    fn js_let_literal_is_not_a_constant() {
        // regression: `let` and `const` are both lexical_declaration in
        // the grammar, so `let n = 42` used to be scoped CONSTANT too
        let src = "let n = 42;\nconst N = 42;\nvar v = 42;\n";
        let spans = semantic_spans("js", src);
        let all = |needle: &str| {
            spans
                .iter()
                .filter(|s| &src[s.start..s.end] == needle)
                .map(|s| s.scope)
                .collect::<Vec<_>>()
        };
        assert!(!all("n").contains(&CONSTANT), "let binding is reassignable");
        assert!(all("N").contains(&CONSTANT), "const literal");
        assert!(!all("v").contains(&CONSTANT), "var binding");
    }

    #[test]
    fn object_literal_keys_js_and_ts() {
        let src = "const o = { msg: 'message', short, 'quoted': 1, [k]: 2 };\nconst { msg: m, short2 } = o;\n";
        for lang in ["js", "ts"] {
            let spans = semantic_spans(lang, src);
            let get = |needle: &str| {
                spans
                    .iter()
                    .filter(|s| &src[s.start..s.end] == needle)
                    .map(|s| s.scope)
                    .collect::<Vec<_>>()
            };
            assert!(
                get("msg").contains(&PROPERTY),
                "{lang}: `{{
 msg: ..}}` key"
            );
            assert!(
                get("short").contains(&PROPERTY),
                "{lang}: `{{ short }}` shorthand"
            );
            assert!(
                !get("quoted").contains(&PROPERTY),
                "{lang}: quoted key keeps its string color"
            );
            // destructuring: bindings, not properties
            let destructured = spans
                .iter()
                .any(|s| s.scope == PROPERTY && s.start > src.find("const {").unwrap());
            assert!(!destructured, "{lang}: patterns stay syntactic");
        }
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
}
