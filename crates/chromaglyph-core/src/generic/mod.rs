//! Generic mode: highlight **any** text of a language, content unknown at
//! build time.
//!
//! Exact mode compiles known text into one rule per occurrence. Generic
//! mode instead synthesizes a tiny state machine out of contextual
//! substitution rules — the idea Cobble / FontWithASyntaxHighlighter
//! proved by hand (706 rules for one language), here generated from a
//! language spec:
//!
//! - **keywords**: rules per keyword over 1:1 letter classes, word
//!   boundaries via the "not an identifier char" class. Keyword *groups*
//!   let one language color `keyword.control` and `storage.type` words
//!   differently, like a real grammar.
//! - **strings**: any number of single-character quote kinds (JS `"`
//!   `'` `` ` ``); the opening quote enters the string state, the closing
//!   quote substitutes into a distinct *end* class so propagation stops
//!   right after it. **Escapes**: `\` plus any one glyph substitute to
//!   body as a pair, so `\"` cannot close the string and `\\` leaves the
//!   state running (the escaped quote leaves the quote class before the
//!   close rule can see it).
//! - **spans**: multi-character delimited states — line comments, block
//!   comments, Python-style triple-quoted strings. A `"""` opener beats
//!   the simple `"` string because openers match longest-first.
//! - **numbers**: a digit after a word boundary opens a number state that
//!   propagates over identifier classes; `.` continues the state only
//!   when a digit follows (`3.14` colors, `1.max` does not swallow
//!   `max`).
//!
//! **Newlines are hard barriers — always, mapped or not.** Browsers lay
//! out `pre` content line by line: every source line is its own shaping
//! run, and contextual rules can never see across a `\n` (a mapped
//! newline glyph changes nothing — the character is consumed as the
//! forced break, so no glyph of this font precedes the next line's first
//! character). The model therefore treats `\n`/`\r` exactly like
//! unmapped characters: states die at line ends, and a keyword at the
//! start of a line has no glyph to anchor on and stays plain (indent it
//! by one space and it colors; the spaces are in-run backtrack). This
//! was verified pixel-exact in a real browser; the earlier v1.1 idea of
//! injecting a newline glyph to keep runs whole does not hold —
//! harfrust shapes the whole text as one run and would happily verify
//! the fiction.
//!
//! Three mechanisms make the states work under real shapers (which apply
//! a contextual lookup left to right, seeing earlier substitutions in
//! the same pass):
//!
//! 1. **Rule order within each first-glyph rule set**: state terminators
//!    before state propagation before state openings (longest input
//!    first) before keywords, so `//` inside a string stays a string and
//!    `"""` wins over `"` at the same position.
//! 2. **Exclusive state classes**: propagation backtracks match only
//!    colored copies, never plain glyphs, so a state always starts at
//!    its delimiter and always ends at its terminator. Because at most
//!    one state can own the glyph before the cursor, the relative order
//!    of *different* states' rules never matters.
//! 3. **Substituted glyphs leave their class**: a rule cannot re-fire
//!    over glyphs it (or an earlier rule) already recolored.
//!
//! Everything stays slot-based: one CPAL palette per theme colors the
//! same rule set under every theme; `@font-palette-values` flips themes
//! with no re-shaping.
//!
//! v1.2 scope: keyword groups, multiple quote kinds per language with
//! escape pairs, line/block comments, triple-quoted strings, numbers —
//! all **line-scoped**; no nested interpolation (`${…}` is colored as
//! string body; triple-quoted spans do not process escapes, so a `\"""`
//! edge there closes textually); a single language per font. Built-in
//! specs: Rust, JavaScript, TypeScript, Python.
//!
//! Layout of this module: `spec` (the spec types), `langs/` (one file
//! per built-in language), `engine` (rule synthesis), `lex` (the
//! reference lexer) and `oracle` (sample texts + shaper verification).

use crate::error::BuildError;

mod engine;
mod langs;
mod lex;
mod oracle;
mod spec;

pub use engine::synthesize;
pub use langs::{
    c::spec as c, cpp::spec as cpp, csharp::spec as csharp, css::spec as css, go::spec as go,
    html::spec as html, java::spec as java, javascript::spec as javascript, lua::spec as lua,
    php::spec as php, python::spec as python, ruby::spec as ruby, rust::spec as rust,
    shell::spec as shell, typescript::spec as typescript,
};
pub use lex::{lex, Tok};
pub use oracle::{sample_texts, verify_generic};
pub use spec::{BlockKind, KeywordGroup, LanguageSpec, SpanKind, StringKind, TagKind};

/// Resolve a language spec by name (the `--generic` CLI argument).
pub fn spec_by_name(name: &str) -> Result<LanguageSpec, BuildError> {
    match name {
        "rust" | "rs" => Ok(rust()),
        "js" | "javascript" | "jsx" | "mjs" | "cjs" => Ok(javascript()),
        "ts" | "typescript" | "tsx" => Ok(typescript()),
        "py" | "python" => Ok(python()),
        "go" | "golang" => Ok(go()),
        "java" => Ok(java()),
        "c" => Ok(c()),
        "cpp" | "c++" | "cxx" => Ok(cpp()),
        "csharp" | "cs" | "c#" => Ok(csharp()),
        "php" => Ok(php()),
        "ruby" | "rb" => Ok(ruby()),
        "lua" => Ok(lua()),
        "shell" | "sh" | "bash" => Ok(shell()),
        "html" | "htm" => Ok(html()),
        "css" => Ok(css()),
        other => Err(BuildError::Tokenize(format!(
            "unknown generic language {other:?}; available: rust, js, ts, python, go, java, c, cpp, csharp, php, ruby, lua, shell, html, css"
        ))),
    }
}
