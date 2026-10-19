//! Python.

use crate::{
    push_field, push_range, SemanticSpan, CONSTANT, FUNCTION, PARAMETER, PROPERTY, TYPE, VARIABLE,
};
use tree_sitter::Node;

/// Python handler.
pub(crate) fn handle_python(node: Node, source: &[u8], spans: &mut Vec<SemanticSpan>) {
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
        // assignment targets: UPPER_CASE names read as constants (how
        // editors treat them; themes that color the scope opt in), every
        // other target is a plain variable binding
        "assignment" => {
            let Some(target) = node.child_by_field_name("left") else {
                return;
            };
            if target.kind() == "identifier" {
                let is_const = target.utf8_text(source).is_ok_and(is_upper_const);
                push_range(target, if is_const { CONSTANT } else { VARIABLE }, spans);
            }
        }
        // type annotations
        "type_identifier" => push_range(node, TYPE, spans),
        // annotations wrap the name in a `type` node: plain names
        // (`str`) are bare identifiers there, generic heads (`list` in
        // list[int]) sit under `generic_type` — neither is a
        // type_identifier, so without this they'd read as variables
        "type" | "generic_type" => {
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    if c.kind() == "identifier" {
                        push_range(c, TYPE, spans);
                    }
                }
            }
        }
        // bare identifiers are variable reads/writes — the semantic blue.
        // Excluded: every position a branch above (or below in dispatch
        // order) already scopes — call/attribute names, parameters,
        // definition names, assignment targets, annotation types, import
        // paths. Parents are handled before children, so an unexcluded
        // identifier here would overwrite those spans. `attribute`
        // excludes only the attribute position: the object of
        // `obj.run()` is itself a variable read and keeps this color
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
                | "type"
                | "generic_type"
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

fn is_upper_const(name: &str) -> bool {
    !name.is_empty()
        && name.chars().any(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_spans;

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
    fn python_assignment_targets_and_annotations() {
        // regression (real report): lowercase assignment targets used to
        // get no span at all — the assignment branch pushed only
        // UPPER_CASE constants while the bare-identifier branch excludes
        // every assignment-left identifier, so `doc`/`other` fell through
        // both. Annotation types (`type > identifier`, not
        // type_identifier) used to fall through to VARIABLE as well
        let src = "def normalize(text: str) -> str:\n\
                    \x20   doc = \"\"\"triple quoted 'won't close this\"\"\"\n\
                    \x20   other = '''also fine \"nested\" here'''\n\
                    \x20   return f\"{doc[:1]}{other[:1]}\"\n\
                    \n\
                    LIMIT = 100_000\n\
                    print(normalize(\"  ok  \"), LIMIT)\n";
        let spans = semantic_spans("python", src);
        let at = |needle: &str, offset: usize| {
            spans
                .iter()
                .find(|s| s.start == offset && &src[s.start..s.end] == needle)
                .map(|s| s.scope)
        };
        // the binding sites before `=` are variables, not just the reads
        assert_eq!(at("doc", src.find("doc =").unwrap()), Some(VARIABLE));
        assert_eq!(at("other", src.find("other =").unwrap()), Some(VARIABLE));
        // the reads inside the f-string interpolations stay variables
        let fstr = src.find("{doc").unwrap() + 1;
        assert_eq!(at("doc", fstr), Some(VARIABLE));
        // annotations read as types, not variables
        assert_eq!(at("str", src.find("str)").unwrap()), Some(TYPE));
        assert_eq!(at("str", src.find("-> str").unwrap() + 3), Some(TYPE));
        assert_eq!(at("text", src.find("text").unwrap()), Some(PARAMETER));
        assert_eq!(at("LIMIT", src.find("LIMIT").unwrap()), Some(CONSTANT));
        assert_eq!(
            at("normalize", src.rfind("normalize").unwrap()),
            Some(FUNCTION)
        );
    }

    #[test]
    fn python_annotated_and_chained_assignments() {
        let src = "x: list[int] = []\nMAX: int = 9\na = b = 3\n";
        let spans = semantic_spans("python", src);
        let all = |needle: &str| {
            spans
                .iter()
                .filter(|s| &src[s.start..s.end] == needle)
                .map(|s| s.scope)
                .collect::<Vec<_>>()
        };
        assert!(all("x").contains(&VARIABLE), "annotated binding");
        assert!(all("list").contains(&TYPE), "generic head");
        assert!(all("int").contains(&TYPE), "generic argument");
        assert!(all("MAX").contains(&CONSTANT), "annotated UPPER_CASE");
        // chained targets are nested assignments; each left gets scoped
        assert!(all("a").contains(&VARIABLE) && all("b").contains(&VARIABLE));
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
                .next_back()
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
}
