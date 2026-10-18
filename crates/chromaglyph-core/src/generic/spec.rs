//! What a language looks like to the rule synthesizer: keyword groups,
//! string kinds, delimited spans, numbers. Colors come from the themes
//! by resolving `scope` like a syntect highlighter would.

/// A keyword group: one TextMate scope, many words. Groups let a language
/// color control flow and declarations differently (VS Code grammars use
/// `keyword.control` vs `storage.type`).
pub struct KeywordGroup {
    pub scope: &'static str,
    pub words: &'static [&'static str],
    /// Require this char right after the word for the keyword to fire
    /// (CSS: a property name only colors when a `:` follows — a selector
    /// word that happens to spell a property stays a selector).
    /// `None` colors at any word boundary, the ordinary behavior.
    pub followed_by: Option<char>,
    /// The group's words appear inside values (CSS `!important`,
    /// `inherit`): their copies carry the value state, so a value exit
    /// (`;`) right after the word still terminates. Meaningful only in
    /// block languages.
    pub value: bool,
}

/// A single-character-quote string kind. Literals never span newlines:
/// browsers shape each source line as its own run (see the module docs).
/// `escapes: false` marks literal kinds (Go raw strings) where a
/// backslash has no meaning — the closing quote closes even after one.
/// `interpolation` enables one level of `${expr}`-style holes: the
/// opener keeps the string color, the expression renders uncolored, the
/// closer resumes the string. Nesting is flat — the first closer wins.
pub struct StringKind {
    pub quote: char,
    pub escapes: bool,
    /// `(opener, closer)`, e.g. JS templates: `Some(("${", '}'))`.
    pub interpolation: Option<(&'static str, char)>,
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

/// A markup tag construct: `<name attr="value">` … `</name>`. Tag
/// names, attribute names and attribute values each carry their own
/// scope; the delimiters themselves stay plain (they are uncolored
/// state carriers — see the engine's interpolation body for the same
/// trick). One state machine per kind, line-scoped like everything
/// else: an unclosed tag dies at the newline.
pub struct TagKind {
    /// opener before the name, e.g. `<`
    pub open: &'static str,
    /// close delimiter, e.g. `>`
    pub close: &'static str,
    /// self-closing variant, e.g. `/>` (matched before `close` —
    /// longest input first); empty = none
    pub self_close: &'static str,
    /// quotes that open a quoted attribute value after `=`
    pub value_quotes: &'static [char],
    /// the attribute separator, `=`
    pub equals: char,
    pub name_scope: &'static str,
    pub attr_scope: &'static str,
    pub value_scope: &'static str,
}

/// A property-sheet construct (CSS-shaped): selector words, `word:`
/// properties, and the value regime. Selectors color after any plain
/// boundary or one of `anchors` (`.`, `:`); the colon after a word
/// enters the value regime — an uncolored state that suppresses
/// selector coloring (and keeps numbers colorable) until one of `ends`
/// (`;`, `{`, `}`). The `{`/`}` block nesting itself cannot be modeled:
/// browsers shape line by line, so a block opened on a previous line is
/// gone — continuation property lines rely on the property keywords'
/// colon lookahead instead.
pub struct BlockKind {
    pub colon: char,
    /// the block opener (`{`) — it exits a value AND anchors the first
    /// property keyword of the block
    pub open: char,
    /// value-regime exits (`;`, `{`, `}`)
    pub ends: &'static [char],
    /// extra backtrack chars for selector opens (`.`, `:`)
    pub anchors: &'static [char],
    pub sel_scope: &'static str,
}

/// What a language looks like to the rule synthesizer. Colors come from
/// themes by resolving `scope` like a syntect highlighter would.
pub struct LanguageSpec {
    pub name: &'static str,
    pub keywords: &'static [KeywordGroup],
    pub strings: &'static [StringKind],
    pub spans: &'static [SpanKind],
    pub tags: &'static [TagKind],
    pub blocks: &'static [BlockKind],
    /// Color `42`, `3.14`, `0x1F` … as numbers.
    pub numbers: bool,
    pub number_scope: &'static str,
    /// Identifier characters beyond ASCII alphanumerics and `_` (JS: `$`).
    pub ident_extra: &'static [char],
}

pub(in crate::generic) fn is_ident(ch: char, extra: &[char]) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || extra.contains(&ch)
}
