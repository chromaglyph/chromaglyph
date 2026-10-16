//! What a language looks like to the rule synthesizer: keyword groups,
//! string kinds, delimited spans, numbers. Colors come from the themes
//! by resolving `scope` like a syntect highlighter would.

/// A keyword group: one TextMate scope, many words. Groups let a language
/// color control flow and declarations differently (VS Code grammars use
/// `keyword.control` vs `storage.type`).
pub struct KeywordGroup {
    pub scope: &'static str,
    pub words: &'static [&'static str],
}

/// A single-character-quote string kind. Literals never span newlines:
/// browsers shape each source line as its own run (see the module docs).
/// `escapes: false` marks literal kinds (Go raw strings) where a
/// backslash has no meaning — the closing quote closes even after one.
pub struct StringKind {
    pub quote: char,
    pub escapes: bool,
    pub scope: &'static str,
}

/// A multi-character delimited span: line comments (empty `close`),
/// block comments, Python-style triple-quoted strings. Every span ends
/// at the closing delimiter **or the end of the line**, whichever comes
/// first — cross-line states cannot exist in browser shaping runs.
pub struct SpanKind {
    pub open: &'static str,
    /// Empty → the span runs to the next newline.
    pub close: &'static str,
    pub scope: &'static str,
}

/// What a language looks like to the rule synthesizer. Colors come from
/// the themes by resolving `scope` like a syntect highlighter would.
pub struct LanguageSpec {
    pub name: &'static str,
    pub keywords: &'static [KeywordGroup],
    pub strings: &'static [StringKind],
    pub spans: &'static [SpanKind],
    /// Color `42`, `3.14`, `0x1F` … as numbers.
    pub numbers: bool,
    pub number_scope: &'static str,
    /// Identifier characters beyond ASCII alphanumerics and `_` (JS: `$`).
    pub ident_extra: &'static [char],
}

pub(in crate::generic) fn is_ident(ch: char, extra: &[char]) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || extra.contains(&ch)
}
