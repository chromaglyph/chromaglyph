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
//!
//! One module per language (`rust`, `javascript`, `python`, `java`), each
//! holding its `handle_*` visitor, its language-specific helpers and its
//! tests. Adding a language is one new module plus a branch in
//! [`semantic_spans`].

mod java;
mod javascript;
mod python;
mod rust;

use java::handle_java;
use javascript::handle_js;
use python::handle_python;
use rust::handle_rust;
use tree_sitter::{Node, Parser, TreeCursor};

/// One semantic span: a byte range in the source plus the TextMate-style
/// dotted scope a theme can color.
#[derive(Clone, Copy, Debug)]
pub struct SemanticSpan {
    pub start: usize,
    pub end: usize,
    pub scope: &'static str,
}

pub(crate) const FUNCTION: &str = "entity.name.function";
pub(crate) const PARAMETER: &str = "variable.parameter";
pub(crate) const TYPE: &str = "entity.name.type";
pub(crate) const CONSTANT: &str = "constant.other";
pub(crate) const ENUM_MEMBER: &str = "entity.name.enum.constant";
pub(crate) const PROPERTY: &str = "variable.other.property";
pub(crate) const VARIABLE: &str = "variable.other.readwrite";

/// Extract semantic spans for a language (`"rust"`, `"js"`/`"ts"`,
/// `"python"`, `"java"` and their aliases); unknown languages yield
/// nothing (the syntactic colors stay).
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

pub(crate) fn push_range(node: Node, scope: &'static str, spans: &mut Vec<SemanticSpan>) {
    spans.push(SemanticSpan {
        start: node.start_byte(),
        end: node.end_byte(),
        scope,
    });
}

pub(crate) fn push_field(
    node: Node,
    field: &str,
    scope: &'static str,
    spans: &mut Vec<SemanticSpan>,
) {
    if let Some(child) = node.child_by_field_name(field) {
        push_range(child, scope, spans);
    }
}
