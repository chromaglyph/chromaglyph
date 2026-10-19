//! Java.

use crate::{
    push_field, push_range, SemanticSpan, CONSTANT, ENUM_MEMBER, FUNCTION, PARAMETER, PROPERTY,
    TYPE, VARIABLE,
};
use tree_sitter::Node;

/// Java handler.
pub(crate) fn handle_java(node: Node, _source: &[u8], spans: &mut Vec<SemanticSpan>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_spans;

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
