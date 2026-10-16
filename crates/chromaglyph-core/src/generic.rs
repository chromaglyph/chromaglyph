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
//! rustybuzz shapes the whole text as one run and would happily verify
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

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::compile::{GlyphPlan, Rule};
use crate::error::BuildError;
use crate::spec::Color;
use crate::theme::ThemeSpec;
use crate::verify::VerifyResult;

// ---- language specs --------------------------------------------------------

/// A keyword group: one TextMate scope, many words. Groups let a language
/// color control flow and declarations differently (VS Code grammars use
/// `keyword.control` vs `storage.type`).
pub struct KeywordGroup {
    pub scope: &'static str,
    pub words: &'static [&'static str],
}

/// A single-character-quote string kind. Literals never span newlines:
/// browsers shape each source line as its own run (see the module docs).
pub struct StringKind {
    pub quote: char,
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

fn is_ident(ch: char, extra: &[char]) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || extra.contains(&ch)
}

/// Built-in Rust spec: keywords per the reference (2018 edition) grammar.
pub fn rust() -> LanguageSpec {
    LanguageSpec {
        name: "rust",
        keywords: &[KeywordGroup {
            scope: "keyword.other.rust",
            words: &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super",
                "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
        }],
        strings: &[StringKind {
            quote: '"',
            scope: "string.quoted.double.rust",
        }],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.rust",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.line.double-slash.rust",
            },
        ],
        numbers: true,
        number_scope: "constant.numeric.rust",
        ident_extra: &[],
    }
}

/// Built-in JavaScript spec (covers JSX; TSX users want [`typescript`]).
pub fn javascript() -> LanguageSpec {
    LanguageSpec {
        name: "javascript",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.js",
                words: &[
                    "as",
                    "await",
                    "break",
                    "case",
                    "catch",
                    "continue",
                    "default",
                    "delete",
                    "do",
                    "else",
                    "export",
                    "finally",
                    "for",
                    "from",
                    "if",
                    "import",
                    "in",
                    "instanceof",
                    "new",
                    "return",
                    "switch",
                    "throw",
                    "try",
                    "typeof",
                    "while",
                    "with",
                    "debugger",
                    "yield",
                ],
            },
            KeywordGroup {
                scope: "storage.type.js",
                words: &[
                    "var", "let", "const", "function", "class", "extends", "super", "static",
                    "get", "set", "async", "of",
                ],
            },
            KeywordGroup {
                scope: "constant.language.js",
                words: &["true", "false", "null", "undefined", "this"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                scope: "string.quoted.double.js",
            },
            StringKind {
                quote: '\'',
                scope: "string.quoted.single.js",
            },
            StringKind {
                quote: '`',
                scope: "string.template.js",
            },
        ],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.js",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.block.js",
            },
        ],
        numbers: true,
        number_scope: "constant.numeric.js",
        ident_extra: &['$'],
    }
}

/// Built-in TypeScript spec: JavaScript plus its type/declaration words.
pub fn typescript() -> LanguageSpec {
    LanguageSpec {
        name: "typescript",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.ts",
                words: &[
                    "as",
                    "await",
                    "break",
                    "case",
                    "catch",
                    "continue",
                    "default",
                    "delete",
                    "do",
                    "else",
                    "export",
                    "finally",
                    "for",
                    "from",
                    "if",
                    "import",
                    "in",
                    "instanceof",
                    "new",
                    "return",
                    "satisfies",
                    "switch",
                    "throw",
                    "try",
                    "typeof",
                    "while",
                    "with",
                    "debugger",
                    "yield",
                ],
            },
            KeywordGroup {
                scope: "storage.type.ts",
                words: &[
                    "var",
                    "let",
                    "const",
                    "function",
                    "class",
                    "extends",
                    "super",
                    "static",
                    "get",
                    "set",
                    "async",
                    "of",
                    "interface",
                    "type",
                    "enum",
                    "implements",
                    "private",
                    "public",
                    "protected",
                    "readonly",
                    "namespace",
                    "declare",
                    "abstract",
                    "keyof",
                    "infer",
                    "is",
                    "asserts",
                    "override",
                    "module",
                    "any",
                    "boolean",
                    "number",
                    "string",
                    "symbol",
                    "object",
                    "never",
                    "unknown",
                    "bigint",
                    "void",
                ],
            },
            KeywordGroup {
                scope: "constant.language.ts",
                words: &["true", "false", "null", "undefined", "this"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                scope: "string.quoted.double.ts",
            },
            StringKind {
                quote: '\'',
                scope: "string.quoted.single.ts",
            },
            StringKind {
                quote: '`',
                scope: "string.template.ts",
            },
        ],
        spans: &[
            SpanKind {
                open: "//",
                close: "",
                scope: "comment.line.double-slash.ts",
            },
            SpanKind {
                open: "/*",
                close: "*/",
                scope: "comment.block.ts",
            },
        ],
        numbers: true,
        number_scope: "constant.numeric.ts",
        ident_extra: &['$'],
    }
}

/// Built-in Python spec: `#` comments, both quote kinds, triple-quoted
/// strings (their opening line — states are line-scoped, see the module
/// docs), f-string prefixes stay plain.
pub fn python() -> LanguageSpec {
    LanguageSpec {
        name: "python",
        keywords: &[
            KeywordGroup {
                scope: "keyword.control.python",
                words: &[
                    "assert", "await", "break", "case", "continue", "del", "elif", "else",
                    "except", "finally", "for", "if", "match", "pass", "raise", "return", "try",
                    "while", "with", "yield",
                ],
            },
            KeywordGroup {
                scope: "storage.type.function.python",
                words: &["def", "lambda"],
            },
            KeywordGroup {
                scope: "storage.type.class.python",
                words: &["class"],
            },
            KeywordGroup {
                scope: "keyword.other.python",
                words: &[
                    "and", "as", "async", "from", "global", "in", "is", "nonlocal", "not", "or",
                ],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                scope: "string.quoted.double.python",
            },
            StringKind {
                quote: '\'',
                scope: "string.quoted.single.python",
            },
        ],
        spans: &[
            SpanKind {
                open: "\"\"\"",
                close: "\"\"\"",
                scope: "string.quoted.double.block.python",
            },
            SpanKind {
                open: "'''",
                close: "'''",
                scope: "string.quoted.single.block.python",
            },
            SpanKind {
                open: "#",
                close: "",
                scope: "comment.line.number-sign.python",
            },
        ],
        numbers: true,
        number_scope: "constant.numeric.python",
        ident_extra: &[],
    }
}

/// Resolve a language spec by name (the `--generic` CLI argument).
pub fn spec_by_name(name: &str) -> Result<LanguageSpec, BuildError> {
    match name {
        "rust" | "rs" => Ok(rust()),
        "js" | "javascript" | "jsx" | "mjs" | "cjs" => Ok(javascript()),
        "ts" | "typescript" | "tsx" => Ok(typescript()),
        "py" | "python" => Ok(python()),
        other => Err(BuildError::Tokenize(format!(
            "unknown generic language {other:?}; available: rust, js, ts, python"
        ))),
    }
}

// ---- slot layout ----------------------------------------------------------
// Slots are palette entries AND nested-lookup indices (see gsub). Opening
// and closing delimiters use distinct slots because a nested single-subst
// lookup maps a glyph to exactly one substitute, but both paint the same
// color. State-bearing slots are never shared between kinds: the state
// class lives on the alt glyphs, so merging (say) a line comment with a
// block comment would also merge their newline semantics.
struct Layout {
    /// slot per keyword group
    kw: Vec<u16>,
    /// slot per string kind (body states need a copy of nearly every glyph)
    str_open: Vec<u16>,
    str_body: Vec<u16>,
    str_close: Vec<u16>,
    num: u16,
    /// slot per span kind (only spans with a close use `span_close`)
    span_open: Vec<u16>,
    span_body: Vec<u16>,
    span_close: Vec<u16>,
    n_slots: u16,
}

fn layout(spec: &LanguageSpec) -> Layout {
    let mut next = 0u16;
    let mut kw = Vec::with_capacity(spec.keywords.len());
    for _ in spec.keywords {
        kw.push(next);
        next += 1;
    }
    let mut str_open = Vec::with_capacity(spec.strings.len());
    let mut str_body = Vec::with_capacity(spec.strings.len());
    let mut str_close = Vec::with_capacity(spec.strings.len());
    for _ in spec.strings {
        str_open.push(next);
        str_body.push(next + 1);
        str_close.push(next + 2);
        next += 3;
    }
    let num = next;
    next += 1;
    let mut span_open = Vec::with_capacity(spec.spans.len());
    let mut span_body = Vec::with_capacity(spec.spans.len());
    let mut span_close = vec![0u16; spec.spans.len()];
    for (j, span) in spec.spans.iter().enumerate() {
        span_open.push(next);
        span_body.push(next + 1);
        next += 2;
        if !span.close.is_empty() {
            span_close[j] = next;
            next += 1;
        }
    }
    Layout {
        kw,
        str_open,
        str_body,
        str_close,
        num,
        span_open,
        span_body,
        span_close,
        n_slots: next,
    }
}

impl Layout {
    fn tok_slot(&self, tok: Tok) -> Option<u16> {
        Some(match tok {
            Tok::Plain => return None,
            Tok::Keyword(g) => self.kw[g],
            Tok::Number => self.num,
            Tok::StrOpen(k) => self.str_open[k],
            Tok::StrBody(k) => self.str_body[k],
            Tok::StrClose(k) => self.str_close[k],
            Tok::SpanOpen(j) => self.span_open[j],
            Tok::SpanBody(j) => self.span_body[j],
            Tok::SpanClose(j) => self.span_close[j],
        })
    }
}

// ---- synthesis -------------------------------------------------------------

/// Synthesize a plan for arbitrary text of `spec`.
///
/// `mappings` is the source font's cmap as (codepoint, glyph id) pairs;
/// `num_source_glyphs` is where new glyph ids start.
pub fn synthesize(
    spec: &LanguageSpec,
    mappings: &[(u32, u16)],
    num_source_glyphs: u16,
    themes: &[ThemeSpec],
) -> Result<GlyphPlan, BuildError> {
    let layout = layout(spec);

    // ---- per-theme colors for the token kinds ----------------------------
    let mut slots: Vec<Vec<Option<Color>>> = vec![Vec::new(); layout.n_slots as usize];
    for (g, group) in spec.keywords.iter().enumerate() {
        slots[layout.kw[g] as usize] = themes.iter().map(|t| t.scope_color(group.scope)).collect();
    }
    for (k, s) in spec.strings.iter().enumerate() {
        let colors: Vec<Option<Color>> = themes.iter().map(|t| t.scope_color(s.scope)).collect();
        slots[layout.str_open[k] as usize] = colors.clone();
        slots[layout.str_body[k] as usize] = colors.clone();
        slots[layout.str_close[k] as usize] = colors;
    }
    slots[layout.num as usize] = themes
        .iter()
        .map(|t| t.scope_color(spec.number_scope))
        .collect();
    for (j, span) in spec.spans.iter().enumerate() {
        let colors: Vec<Option<Color>> = themes.iter().map(|t| t.scope_color(span.scope)).collect();
        slots[layout.span_open[j] as usize] = colors.clone();
        slots[layout.span_body[j] as usize] = colors.clone();
        if !span.close.is_empty() {
            slots[layout.span_close[j] as usize] = colors;
        }
    }
    let theme_fgs: Vec<Color> = themes.iter().map(|t| t.foreground).collect();

    // ---- glyph classes ----------------------------------------------------
    // Every mapped glyph is partitioned by the role of its character:
    // delimiter chars (quotes, span open/close chars, the dot) get 1:1
    // classes, keyword letters get 1:1 classes (keyword inputs), digits
    // get one class (number inputs), other identifier chars share a
    // class, and the rest (including .notdef) is the "not a word char"
    // class used for keyword boundaries.
    let mut char_class: HashMap<char, u16> = HashMap::new();
    let mut next_class: u16 = 3; // 1 = not-a-word, 2 = identifier (below)
    for s in spec.strings {
        char_class.entry(s.quote).or_insert_with(|| {
            let id = next_class;
            next_class += 1;
            id
        });
    }
    for span in spec.spans {
        for ch in span.open.chars().chain(span.close.chars()) {
            char_class.entry(ch).or_insert_with(|| {
                let id = next_class;
                next_class += 1;
                id
            });
        }
    }
    let c_dot = char_class.entry('.').or_insert_with(|| {
        let id = next_class;
        next_class += 1;
        id
    });
    let c_dot = *c_dot;
    // the escape char drives two-glyph pair rules inside strings, so it
    // needs its own class too (it stops being a plain keyword boundary,
    // like the dot)
    let c_esc = char_class.entry('\\').or_insert_with(|| {
        let id = next_class;
        next_class += 1;
        id
    });
    let c_esc = *c_esc;
    let c_nl = next_class;
    next_class += 1;
    // .notdef is its own class, referenced by no rule: an unmapped char in
    // the oracle (and a font-fallback run break in browsers) must NOT count
    // as a word boundary a keyword could anchor on
    let c_notdef = next_class;
    next_class += 1;
    let c_digit = next_class;
    next_class += 1;
    let keyword_letters: Vec<char> = {
        let mut set: Vec<char> = spec
            .keywords
            .iter()
            .flat_map(|g| g.words.iter())
            .flat_map(|w| w.chars())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        set.sort_unstable();
        set
    };
    let first_letter_class = next_class;
    let letter_class: HashMap<char, u16> = keyword_letters
        .iter()
        .enumerate()
        .map(|(i, &c)| (c, first_letter_class + i as u16))
        .collect();
    next_class += keyword_letters.len() as u16;
    // state classes (exclusive sets of colored copies)
    let str_state: Vec<u16> = (0..spec.strings.len())
        .map(|_| {
            let id = next_class;
            next_class += 1;
            id
        })
        .collect();
    let span_state: Vec<u16> = (0..spec.spans.len())
        .map(|_| {
            let id = next_class;
            next_class += 1;
            id
        })
        .collect();
    let c_num_state = next_class;
    next_class += 1;
    let c_end_state = next_class;

    const C_NOTWORD: u16 = 1;
    const C_IDENT: u16 = 2;

    let mut class_of: HashMap<u16, u16> = HashMap::new();
    class_of.insert(0, c_notdef); // .notdef: never a context, never substituted
    let mut sorted = mappings.to_vec();
    sorted.sort_unstable();
    let mut newline_gid: Option<u16> = None;
    for &(cp, gid) in &sorted {
        let Some(ch) = char::from_u32(cp) else {
            continue;
        };
        if ch == '\n' || ch == '\r' {
            if newline_gid.is_none() {
                class_of.insert(gid, c_nl);
                newline_gid = Some(gid);
            }
            continue;
        }
        if class_of.contains_key(&gid) {
            continue; // first mapping of a glyph decides its class
        }
        let class = if let Some(&id) = char_class.get(&ch) {
            id
        } else if ch.is_ascii_digit() {
            c_digit
        } else if is_ident(ch, spec.ident_extra) {
            letter_class.get(&ch).copied().unwrap_or(C_IDENT)
        } else {
            C_NOTWORD
        };
        class_of.insert(gid, class);
    }
    let quote_gid: Vec<Option<u16>> = spec
        .strings
        .iter()
        .map(|s| char_gid(mappings, s.quote))
        .collect();
    let span_delim_gids: Vec<Vec<Option<u16>>> = spec
        .spans
        .iter()
        .map(|span| {
            span.open
                .chars()
                .chain(span.close.chars())
                .map(|c| char_gid(mappings, c))
                .collect()
        })
        .collect();
    let digit_gid = char_gid(mappings, '0');

    // ---- alt glyphs -------------------------------------------------------
    let mut gids: Vec<u16> = class_of
        .keys()
        .copied()
        .filter(|&g| g != 0)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    gids.sort_unstable();

    let mut alt_glyphs: Vec<(u16, u16, u16)> = Vec::new();
    let mut next_new = num_source_glyphs;
    fn push_alt(
        next_new: &mut u16,
        alt_glyphs: &mut Vec<(u16, u16, u16)>,
        base: u16,
        pal: u16,
        class: u16,
        class_of: &mut HashMap<u16, u16>,
    ) -> u16 {
        let alt = *next_new;
        *next_new += 1;
        alt_glyphs.push((base, pal, alt));
        class_of.insert(alt, class);
        alt
    }
    // keyword letters (keyword copies keep the base class so keyword
    // rules stay form-insensitive); a letter used by several groups gets
    // one alt per group slot
    for (g, group) in spec.keywords.iter().enumerate() {
        let letters: Vec<char> = {
            let mut v: Vec<char> = group
                .words
                .iter()
                .flat_map(|w| w.chars())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            v.sort_unstable();
            v
        };
        for c in letters {
            if let Some(gid) = char_gid(mappings, c) {
                let class = class_of[&gid];
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.kw[g],
                    class,
                    &mut class_of,
                );
            }
        }
    }
    // string delimiters + bodies (every mapped glyph gets a body copy,
    // including the quote itself — the escape pair rule substitutes an
    // escaped quote to body; a plain quote still closes via rule order)
    for (k, _s) in spec.strings.iter().enumerate() {
        let Some(q) = quote_gid[k] else { continue };
        push_alt(
            &mut next_new,
            &mut alt_glyphs,
            q,
            layout.str_open[k],
            str_state[k],
            &mut class_of,
        );
        push_alt(
            &mut next_new,
            &mut alt_glyphs,
            q,
            layout.str_close[k],
            c_end_state,
            &mut class_of,
        );
        for &gid in &gids {
            let class = class_of[&gid];
            // the quote glyph gets a body copy too: an ESCAPED quote is
            // substituted by the pair rule and must leave the quote class
            // (a plain quote still closes — that is rule order, not alts)
            let body = class != c_notdef && class != c_nl;
            if body {
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.str_body[k],
                    str_state[k],
                    &mut class_of,
                );
            }
        }
    }
    // span delimiters + bodies (openers deduplicated per glyph — `"""`
    // opens over the same quote glyph three times)
    for (j, span) in spec.spans.iter().enumerate() {
        let mut seen: HashSet<u16> = HashSet::new();
        for gid in span_delim_gids[j]
            .iter()
            .take(span.open.chars().count())
            .copied()
            .flatten()
        {
            if seen.insert(gid) {
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.span_open[j],
                    span_state[j],
                    &mut class_of,
                );
            }
        }
        if !span.close.is_empty() {
            let open_len = span.open.chars().count();
            let mut seen_close: HashSet<u16> = HashSet::new();
            for gid in span_delim_gids[j][open_len..].iter().copied().flatten() {
                if seen_close.insert(gid) {
                    push_alt(
                        &mut next_new,
                        &mut alt_glyphs,
                        gid,
                        layout.span_close[j],
                        c_end_state,
                        &mut class_of,
                    );
                }
            }
        }
        let _line = span.close.is_empty();
        for &gid in &gids {
            let class = class_of[&gid];
            if class != c_notdef && class != c_nl {
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.span_body[j],
                    span_state[j],
                    &mut class_of,
                );
            }
        }
    }
    // number glyphs: digits, identifier chars, letters, dot
    if spec.numbers && digit_gid.is_some() {
        for &gid in &gids {
            let class = class_of[&gid];
            let wordish = class == c_digit
                || class == C_IDENT
                || class == c_dot
                || (class >= first_letter_class
                    && class < first_letter_class + keyword_letters.len() as u16);
            if wordish {
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.num,
                    c_num_state,
                    &mut class_of,
                );
            }
        }
    }

    // ---- newline: no glyph, on purpose -----------------------------------
    // Browsers shape each source line as its own run (the `\n` is consumed
    // as the forced break), so mapping a newline glyph would change
    // nothing in a real renderer — rustybuzz alone would shape across it
    // and "verify" behavior no browser has. `plain_glyphs` stays empty;
    // the model treats `\n`/`\r` as hard barriers (class c_nl / notdef
    // participate in no rule), exactly like unmapped characters.
    let plain_glyphs: Vec<(u32, u16)> = Vec::new();

    // ---- rules ------------------------------------------------------------
    // Order inside a first-glyph rule set decides who wins at a position:
    // state terminators (longest input first), state propagation, state
    // openings (longest input first, so `"""` beats `"`), number opens,
    // keywords. Different states' rules never conflict: the backtrack
    // glyph belongs to exactly one state class.
    let mut by_class: BTreeMap<u16, Vec<Rule>> = BTreeMap::new();
    fn prop(
        by_class: &mut BTreeMap<u16, Vec<Rule>>,
        class: u16,
        state: u16,
        pal: u16,
        lookahead: Vec<u16>,
    ) {
        by_class.entry(class).or_default().push(Rule {
            backtrack: vec![state],
            input: vec![class],
            lookahead,
            pal,
        });
    }

    // terminators, precomputed per first class, longest input first
    let mut terminators: HashMap<u16, Vec<Rule>> = HashMap::new();
    for (j, span) in spec.spans.iter().enumerate() {
        if span.close.is_empty() {
            continue;
        }
        let Some(&class) = span.close.chars().next().and_then(|c| char_class.get(&c)) else {
            continue;
        };
        let input: Vec<u16> = span.close.chars().map(|c| char_class[&c]).collect();
        terminators.entry(class).or_default().push(Rule {
            backtrack: vec![span_state[j]],
            input,
            lookahead: vec![],
            pal: layout.span_close[j],
        });
    }
    for (k, s) in spec.strings.iter().enumerate() {
        let Some(&class) = char_class.get(&s.quote) else {
            continue;
        };
        terminators.entry(class).or_default().push(Rule {
            backtrack: vec![str_state[k]],
            input: vec![class],
            lookahead: vec![],
            pal: layout.str_close[k],
        });
    }
    for rules in terminators.values_mut() {
        rules.sort_by_key(|r| std::cmp::Reverse(r.input.len()));
    }

    // openings, precomputed per first class, longest input first
    let mut openers: HashMap<u16, Vec<Rule>> = HashMap::new();
    for (j, span) in spec.spans.iter().enumerate() {
        let Some(&class) = span.open.chars().next().and_then(|c| char_class.get(&c)) else {
            continue;
        };
        let input: Vec<u16> = span.open.chars().map(|c| char_class[&c]).collect();
        openers.entry(class).or_default().push(Rule {
            backtrack: vec![],
            input,
            lookahead: vec![],
            pal: layout.span_open[j],
        });
    }
    for (k, s) in spec.strings.iter().enumerate() {
        let Some(&class) = char_class.get(&s.quote) else {
            continue;
        };
        openers.entry(class).or_default().push(Rule {
            backtrack: vec![],
            input: vec![class],
            lookahead: vec![],
            pal: layout.str_open[k],
        });
    }
    for rules in openers.values_mut() {
        rules.sort_by_key(|r| std::cmp::Reverse(r.input.len()));
    }

    let present: Vec<u16> = {
        let mut v: Vec<u16> = class_of
            .values()
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        v.sort_unstable();
        v
    };
    // number propagation covers exactly the word-ish classes: digits,
    // identifier chars, keyword letters, dot (with a digit lookahead).
    // Delimiter classes have no number rule — the number state dies at a
    // quote or comment delimiter, which then opens its own state.
    let num_body = |class: u16| -> bool {
        spec.numbers
            && digit_gid.is_some()
            && (class == c_digit
                || class == C_IDENT
                || (class >= first_letter_class
                    && class < first_letter_class + keyword_letters.len() as u16))
    };
    // keyword boundary class: plain punctuation only. A newline can never
    // anchor a keyword — the previous line is a different shaping run in
    // every browser, so a column-0 keyword has no in-run glyph before it
    let boundaries = [C_NOTWORD];

    for &class in &present {
        if let Some(rules) = terminators.get(&class) {
            by_class
                .entry(class)
                .or_default()
                .extend(rules.iter().cloned());
        }
        // escape pairs: inside a string, `\` plus ANY one glyph substitute
        // to body in a single rule. The shaper then walks past an escaped
        // quote that has already left the quote class, so `\"` cannot
        // close the string and `\\` leaves the state running. Must precede
        // the single-glyph propagation below so the pair wins at the
        // backslash; spans (comments, triple quotes) get no pairs —
        // backslash has no meaning there and would suppress real closers
        if class == c_esc {
            for (k, _s) in spec.strings.iter().enumerate() {
                if quote_gid[k].is_none() {
                    continue;
                }
                for &x in &present {
                    if x == c_notdef || x == c_nl {
                        continue;
                    }
                    by_class.entry(class).or_default().push(Rule {
                        backtrack: vec![str_state[k]],
                        input: vec![c_esc, x],
                        lookahead: vec![],
                        pal: layout.str_body[k],
                    });
                }
            }
        }
        for (j, _span) in spec.spans.iter().enumerate() {
            // every span dies at the newline — line-scoped by browser
            // necessity, line comment and block comment alike
            if class != c_notdef && class != c_nl {
                prop(
                    &mut by_class,
                    class,
                    span_state[j],
                    layout.span_body[j],
                    vec![],
                );
            }
        }
        for (k, s) in spec.strings.iter().enumerate() {
            let Some(&quote_class) = char_class.get(&s.quote) else {
                continue;
            };
            if class != quote_class && class != c_notdef && class != c_nl {
                prop(
                    &mut by_class,
                    class,
                    str_state[k],
                    layout.str_body[k],
                    vec![],
                );
            }
        }
        if spec.numbers && class == c_dot {
            // `.` continues a number only when a digit follows: 3.14 yes,
            // 1.max stops before the dot
            prop(&mut by_class, class, c_num_state, layout.num, vec![c_digit]);
        }
        if num_body(class) {
            prop(&mut by_class, class, c_num_state, layout.num, vec![]);
        }
        if let Some(rules) = openers.get(&class) {
            by_class
                .entry(class)
                .or_default()
                .extend(rules.iter().cloned());
        }
        // a number opens after a word boundary (or at a line start)
        if class == c_digit && spec.numbers {
            for &bt in &boundaries {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![bt],
                    input: vec![c_digit],
                    lookahead: vec![],
                    pal: layout.num,
                });
            }
        }
        // keywords, attached to the rule set of their first letter's
        // class: four boundary combinations (plain punctuation / newline
        // on each side)
        for (g, group) in spec.keywords.iter().enumerate() {
            for kw in group.words {
                let Some(first) = kw.chars().next() else {
                    continue;
                };
                if letter_class.get(&first) != Some(&class) {
                    continue;
                }
                let input: Vec<u16> = kw.chars().map(|c| letter_class[&c]).collect();
                for &bt in &boundaries {
                    for &la in &boundaries {
                        by_class.entry(class).or_default().push(Rule {
                            backtrack: vec![bt],
                            input: input.clone(),
                            lookahead: vec![la],
                            pal: layout.kw[g],
                        });
                    }
                }
            }
        }
    }
    let rules: Vec<Rule> = by_class.into_values().flatten().collect();

    Ok(GlyphPlan {
        cells: Vec::new(),
        alt_glyphs,
        class_of,
        rules,
        slots,
        theme_fgs,
        num_source_glyphs,
        plain_glyphs,
        block_ranges: Vec::new(),
        uncolored_cells: 0,
        warnings: Vec::new(),
    })
}

fn char_gid(mappings: &[(u32, u16)], ch: char) -> Option<u16> {
    mappings
        .iter()
        .find(|&&(cp, _)| cp == ch as u32)
        .map(|&(_, gid)| gid)
        .filter(|&gid| gid != 0)
}

// ---- reference lexer + oracle ---------------------------------------------

/// Reference lexer output per character; the font must reproduce exactly
/// this when shaped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Plain,
    /// group index within `LanguageSpec::keywords`
    Keyword(usize),
    Number,
    /// string kind index within `LanguageSpec::strings`
    StrOpen(usize),
    StrBody(usize),
    StrClose(usize),
    /// span kind index within `LanguageSpec::spans`
    SpanOpen(usize),
    SpanBody(usize),
    SpanClose(usize),
}

/// Highlight `text` per the spec, one [`Tok`] per char, **line-scoped**.
///
/// `mapped` says whether the *output* font has a glyph for a char; it
/// models the shaper's real capabilities. Newlines are hard barriers
/// even when a glyph exists for them: browsers shape each source line as
/// its own run, so no state and no keyword anchor can cross a `\n`.
/// Unmapped chars (emoji, …) break runs the same way. Span openers match
/// longest-first, so `"""` beats `"`.
pub fn lex(spec: &LanguageSpec, text: &str, mapped: &dyn Fn(char) -> bool) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = vec![Tok::Plain; chars.len()];
    // a word boundary glyph: mapped, not an identifier char, not a
    // newline, and not one of the delimiter chars that live in their own
    // classes (quotes, span delimiters, the dot, the escape char)
    let boundary = |c: char| {
        mapped(c)
            && c != '\n'
            && c != '\r'
            && !is_ident(c, spec.ident_extra)
            && !spec.strings.iter().any(|s| s.quote == c)
            && !spec
                .spans
                .iter()
                .any(|s| s.open.contains(c) || s.close.contains(c))
            && c != '.'
            && c != '\\'
    };
    let starts = |i: usize, delim: &str| -> bool {
        let cs: Vec<char> = delim.chars().collect();
        chars[i..].starts_with(&cs)
    };
    let all_mapped = |s: &str| s.chars().all(mapped);
    // longest opener first: `"""` must win over `"` at the same position
    let mut span_order: Vec<usize> = (0..spec.spans.len()).collect();
    span_order.sort_by_key(|&j| std::cmp::Reverse(spec.spans[j].open.chars().count()));

    let mut i = 0;
    'outer: while i < chars.len() {
        for &j in &span_order {
            let span = &spec.spans[j];
            if !starts(i, span.open) || !all_mapped(span.open) {
                continue;
            }
            let n = span.open.chars().count();
            for tok in out.iter_mut().take(i + n).skip(i) {
                *tok = Tok::SpanOpen(j);
            }
            i += n;
            if span.close.is_empty() {
                while i < chars.len() {
                    let c = chars[i];
                    if c == '\n' || c == '\r' || !mapped(c) {
                        break;
                    }
                    out[i] = Tok::SpanBody(j);
                    i += 1;
                }
            } else {
                while i < chars.len() {
                    if starts(i, span.close) && all_mapped(span.close) {
                        let n = span.close.chars().count();
                        for tok in out.iter_mut().take(i + n).skip(i) {
                            *tok = Tok::SpanClose(j);
                        }
                        i += n;
                        break;
                    }
                    let c = chars[i];
                    if !mapped(c) || c == '\n' || c == '\r' {
                        break; // barrier or line end: state dies
                    }
                    out[i] = Tok::SpanBody(j);
                    i += 1;
                }
            }
            continue 'outer;
        }
        for (k, s) in spec.strings.iter().enumerate() {
            if chars[i] != s.quote || !mapped(s.quote) {
                continue;
            }
            out[i] = Tok::StrOpen(k);
            i += 1;
            while i < chars.len() {
                let c = chars[i];
                if c == s.quote && mapped(c) {
                    out[i] = Tok::StrClose(k);
                    i += 1;
                    break;
                }
                // escape pair: `\` + any mapped non-newline glyph becomes
                // body in one step (mirrors the font's pair rule), so the
                // escaped quote never closes and `\\` keeps the state
                if c == '\\'
                    && mapped(c)
                    && i + 1 < chars.len()
                    && mapped(chars[i + 1])
                    && chars[i + 1] != '\n'
                    && chars[i + 1] != '\r'
                {
                    out[i] = Tok::StrBody(k);
                    out[i + 1] = Tok::StrBody(k);
                    i += 2;
                    continue;
                }
                if !mapped(c) || c == '\n' || c == '\r' {
                    break; // barrier or line end: state dies
                }
                out[i] = Tok::StrBody(k);
                i += 1;
            }
            continue 'outer;
        }
        if spec.numbers && chars[i].is_ascii_digit() && mapped(chars[i]) {
            let start = i;
            if start > 0 && boundary(chars[start - 1]) {
                while i < chars.len() && is_ident(chars[i], spec.ident_extra) {
                    out[i] = Tok::Number;
                    i += 1;
                }
                // fractional part: a dot continues the number only when
                // a digit follows it (mirrors the font's lookahead rule)
                while i < chars.len() {
                    let continues = is_ident(chars[i], spec.ident_extra)
                        || (chars[i] == '.'
                            && i + 1 < chars.len()
                            && chars[i + 1].is_ascii_digit());
                    if !continues {
                        break;
                    }
                    out[i] = Tok::Number;
                    i += 1;
                }
            } else {
                i += 1; // digit inside an identifier is not a number
            }
            continue;
        }
        if is_ident(chars[i], spec.ident_extra) {
            let start = i;
            while i < chars.len() && is_ident(chars[i], spec.ident_extra) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let group = spec
                .keywords
                .iter()
                .position(|g| g.words.contains(&word.as_str()));
            if let Some(g) = group {
                if start > 0
                    && boundary(chars[start - 1])
                    && i < chars.len()
                    && boundary(chars[i])
                    && word.chars().all(mapped)
                {
                    for tok in out.iter_mut().take(i).skip(start) {
                        *tok = Tok::Keyword(g);
                    }
                }
            }
            continue;
        }
        i += 1;
    }
    out
}

/// Curated texts per language, plus structural samples derived from the
/// spec and deterministic-random assemblies; the oracle must hold on all.
pub fn sample_texts(spec: &LanguageSpec) -> Vec<String> {
    let mut samples = curated(spec.name);
    let words: Vec<&str> = spec
        .keywords
        .iter()
        .flat_map(|g| g.words.iter().copied())
        .collect();
    // keyword glue and boundaries with the first two words of the spec
    if let (Some(&a), Some(&b)) = (words.first(), words.get(1)) {
        samples.push(format!("{a}{a} {a}x x{a} _{a} {a}_ {b}{a} a {a} b"));
        samples.push(format!("{a}\n{b} {a}{b}\n{a} at_line_start\nplain {a}"));
    }
    // delimiter soup: quote runs and every opener/close adjacency
    let mut soup = String::new();
    for s in spec.strings {
        for _ in 0..3 {
            soup.push(s.quote);
        }
    }
    for sp in spec.spans {
        soup.push_str(sp.open);
        soup.push_str(sp.close);
    }
    samples.push(soup);
    // an unmapped char (emoji) breaks the shaping run: the string dies
    // there and a later quote opens a fresh one
    if let Some(q) = spec.strings.first().map(|s| s.quote) {
        samples.push(format!("a{q}b\u{1F680}c{q}d{q}"));
    }

    // deterministic-random assembly from spec-derived pieces
    let mut pieces: Vec<String> = vec![
        " ".into(),
        "\n".into(),
        "=".into(),
        ";".into(),
        "x".into(),
        "_y1".into(),
        "42".into(),
        "3.14".into(),
        "0x1F".into(),
        ".".into(),
        "e\u{1F680}f".into(),
    ];
    for w in &words {
        pieces.push((*w).to_string());
    }
    for s in spec.strings {
        pieces.push(s.quote.to_string());
        pieces.push(format!("{}ab{}", s.quote, s.quote));
        pieces.push(format!("{}a\nb{}", s.quote, s.quote));
        // escape pairs: `\"` must not close, `\\` must not eat the closer
        pieces.push(format!("{}a\\\"b{}", s.quote, s.quote));
        pieces.push(format!("{}\\\\{}", s.quote, s.quote));
        pieces.push(format!("{}x\\", s.quote)); // dangling escape
    }
    pieces.push("\\".into());
    pieces.push("\\\\".into());
    for sp in spec.spans {
        pieces.push(sp.open.to_string());
        if sp.close.is_empty() {
            pieces.push(format!("{} c\n", sp.open));
        } else {
            pieces.push(sp.close.to_string());
            pieces.push(format!("{} b {}", sp.open, sp.close));
            pieces.push(format!("{}a\nb{}", sp.open, sp.close));
        }
    }
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..400 {
        let n = 4 + (next() as usize % 24);
        let s: String = (0..n)
            .map(|_| pieces[next() as usize % pieces.len()].clone())
            .collect();
        samples.push(s);
    }
    samples
}

fn curated(name: &str) -> Vec<String> {
    match name {
        "javascript" => [
            "let x = 1;",
            "// comment\nfunction f() {}",
            "const s = 'fn // let';",
            "// 'quoted' comment\nlet y;",
            "let s = \"a'b`c\";",
            "let q = 'it\\'s'; let r = \"a\\\"b\";",
            "let t = `x ${y} z`;",
            "let u = `tick \\` inside`;",
            "`multi\nline\ntemplate`",
            "'' \"\" ``",
            "'a\"b' \"c'd\" `e\"f'g`",
            "''''''",
            "//////",
            "/* block */ let x;",
            "/* multi\nline\ncomment */ const y;",
            "/* unterminated block let function",
            "a/*b'c*/d'e",
            "*/*/*/**/",
            "let a = 42; let b = 3.14; let c = 0x1F; let d = 1e5;",
            "1..toString() x1 12x 0b101 1_000",
            "letletlet let_ let1 1let",
            "async function* gen() { yield await x; }",
            "class A extends B { static m() { return this; } }",
            "import { a } from 'mod'; export default A;",
            "null undefined true false this",
            "\nfunction at_line_start\nlet at_line_start\n42 at_line_start",
            "let $x = $require('y'); $let let$ $let$",
            "let s = '\"' // '\"\n`//` \"'`",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        "typescript" => [
            "interface I { x: number; }",
            "type T = Array<string>;",
            "const x: Array<number> = [1, 2];",
            "enum E { A, B }",
            "abstract class C implements I { private readonly x?: string; }",
            "function f<T extends object>(v: T): T { return v; }",
            "declare module 'm' { export const x: 1; }",
            "const y = x as unknown as string;",
            "const z = w satisfies T;",
            "let a: any = null; let b: void; let c: unknown;",
            "type K = keyof O;",
            "is x string? asserts v is T: infer U;",
            "// comment\nconst s: string = `t ${x}`;",
            "/* multi\nline */ let t: boolean;",
            "x = 42; y = 3.14; z = 0xFF;",
            "namespace N { export function g(): never { throw 1; } }",
            "class D extends C implements I { override m() {} }",
            "\ninterface at_line_start\nconst at_line_start\n42 at_line_start",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        "python" => [
            "x = 1",
            "# comment\ndef main():\n    pass",
            "s = \"fn # let\"",
            "s = 'single # \"q\"'",
            "t = 'it\\'s'; u = 'a\\\\b'",
            "\"\"\"triple\nline\nstring\"\"\"",
            "'''single triple with \" inside'''",
            "\"\"\"a\"\"b\"\"\"",
            "''''''",
            "'''''",
            "f\"interp {x} end\"",
            "def f():\n    \"\"\"Docstring.\"\"\"\n    return 1",
            "'''unterminated triple",
            "\"unterminated string def class",
            "# unterminated comment def if",
            "\"\"\"\"quoted\"\"\"\"",
            "x = 42; y = 3.14; z = 0x1F; w = 1_000",
            "class C(Base):\n    def m(self):\n        return self",
            "if x and not y or z in w is v:\n    pass",
            "g = lambda x: x + 1",
            "import os.path as p\nfrom x import (a, b)",
            "async def f():\n    await g()",
            "match x:\n    case 1:\n        pass",
            "global g; nonlocal n; del x; assert y",
            "\ndef at_line_start\nx = at_line_start\n42 at_line_start",
            "a#b'c\"d'''e",
            "deffn def defx xdef _def def_",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        // rust keeps the v1.1 curated set verbatim
        _ => [
            "let x = 1;",
            "// comment\nfn main() {}",
            "let s = \"fn // let\";",
            "// \"quoted\" comment\nlet y;",
            "\"\"\"",
            "//////",
            "fnfn fn fnx xfn _fn fn_",
            "a\"b\"c\"d",
            "/\n// x\n/",
            "let s = \"\";",
            "let esc = \"quote: \\\" back: \\\\\";",
            "struct S; impl S { fn f() -> Self { Self } }",
            "/* block */ let x;",
            "/* multi\nline\ncomment */ let y;",
            "/* unterminated block let fn",
            "a/*b\"c*/d\"e",
            "*/*/*/**/",
            "let a = 42; let b = 3.14; let c = 0x1F_ff_u32; let d = 1_000;",
            "1.max(2).min(3)",
            "x1 12x a1b2 0b1010 1e5 1.2.3 42.",
            "\nfn at_line_start\nlet at_line_start\n42 at_line_start",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    }
}

/// The generic-mode oracle: shape every sample with the produced font and
/// diff each glyph against the reference lexer's expectation.
pub fn verify_generic(
    ttf: &[u8],
    spec: &LanguageSpec,
    alt_of: &HashMap<(u16, u16), u16>,
    charmap: &dyn Fn(char) -> Option<u16>,
    samples: &[String],
) -> VerifyResult {
    let layout = layout(spec);
    let mut result = VerifyResult::ok();
    let Some(face) = rustybuzz::Face::from_slice(ttf, 0) else {
        result.fail("rustybuzz cannot parse the produced font".into());
        return result;
    };
    for sample in samples {
        let toks = lex(spec, sample, &|ch| charmap(ch).is_some());
        // (byte offset, expected glyph id) per char
        let expected: Vec<(usize, u16)> = sample
            .char_indices()
            .zip(&toks)
            .map(|((byte, ch), &tok)| {
                let gid = charmap(ch).unwrap_or(0);
                let want = match layout.tok_slot(tok) {
                    None => gid,
                    Some(pal) => alt_of.get(&(gid, pal)).copied().unwrap_or(gid),
                };
                (byte, want)
            })
            .collect();

        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(sample);
        let infos = rustybuzz::shape(&face, &[], buffer).glyph_infos().to_vec();

        let mut cells = expected.iter();
        let mut pending = cells.next().copied();
        for info in infos {
            let cluster = info.cluster as usize;
            let Some(&(byte, want)) = pending.as_ref() else {
                result.fail(format!("{sample:?}: extra glyph at cluster {cluster}"));
                break;
            };
            if cluster < byte {
                continue; // merged/hidden cluster artifact
            }
            if cluster > byte {
                result.fail(format!("{sample:?}: char at byte {byte} produced no glyph"));
                break;
            }
            result.glyphs_checked += 1;
            let got = info.glyph_id as u16;
            if got != want {
                let ch: String = sample[byte..].chars().take(1).collect();
                result.fail(format!(
                    "{sample:?} byte {byte} ({ch:?}): got glyph {got}, expected {want}"
                ));
            }
            pending = cells.next().copied();
        }
        if let Some((byte, _)) = pending {
            result.fail(format!("{sample:?}: char at byte {byte} produced no glyph"));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        // everything mapped except the barrier chars that shape no glyph
        // in a browser (newlines are barriers regardless — see module docs)
        lex(&rust(), s, &|c| c != '\n' && c != '\r')
    }

    #[test]
    fn lex_keywords_and_boundaries() {
        // keyword between punctuation colors; at text start it cannot
        // (no glyph exists for the shaper to backtrack on)
        let t = toks("a let b");
        assert_eq!(&t[2..5], &[Tok::Keyword(0); 3]);
        let t = toks("let x");
        assert_eq!(&t[0..3], &[Tok::Plain; 3], "text-start keyword stays plain");
        // inside an identifier it is never a keyword
        let t = toks("a let_x b");
        assert_eq!(&t[2..6], &[Tok::Plain; 4]);
        // adjacent to delimiter classes it stays plain (those glyphs live
        // outside the not-a-word-char class)
        let t = toks("a let\" b");
        assert_eq!(&t[2..5], &[Tok::Plain; 3]);
    }

    #[test]
    fn lex_line_start_keywords_stay_plain() {
        // every source line is its own shaping run in a browser: a
        // column-0 keyword has no in-run glyph to anchor on — even when
        // the font maps `\n` (pixel-verified in a real browser)
        let t = lex(&rust(), "a\nfn b", &|_| true);
        assert_eq!(&t[2..4], &[Tok::Plain; 2], "line start: no anchor");
        // one space of indentation and the space glyph anchors it
        let t = lex(&rust(), "a\n fn b", &|_| true);
        assert_eq!(&t[3..5], &[Tok::Keyword(0); 2]);
    }

    #[test]
    fn lex_newline_is_a_barrier_even_when_mapped() {
        // states die at the newline; the next line starts a fresh scan
        let t = lex(&rust(), "\"a\nfn let", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(0));
        assert_eq!(t[1], Tok::StrBody(0));
        assert_eq!(t[2], Tok::Plain);
        assert_eq!(&t[3..9], &[Tok::Plain; 6]);
    }

    #[test]
    fn lex_strings_and_comments() {
        let t = toks("a \"b // c\" d");
        assert_eq!(t[2], Tok::StrOpen(0));
        assert_eq!(&t[3..9], &[Tok::StrBody(0); 6]); // `//` inside a string stays string
        assert_eq!(t[9], Tok::StrClose(0));
        let t = toks("// \"q\" x\nfn y");
        assert_eq!(&t[0..2], &[Tok::SpanOpen(0); 2]);
        assert_eq!(&t[2..8], &[Tok::SpanBody(0); 6]); // quote inside a comment stays comment
        assert_eq!(t[8], Tok::Plain); // newline ends the comment
        assert_eq!(&t[9..11], &[Tok::Plain; 2], "line-start keyword stays plain");
    }

    #[test]
    fn lex_block_comments() {
        // block comments are line-scoped: continuation lines start plain
        let t = toks("a /* b\n c */ d");
        assert_eq!(&t[2..4], &[Tok::SpanOpen(1); 2]);
        assert_eq!(&t[4..6], &[Tok::SpanBody(1); 2]); // " b"
        assert_eq!(t[6], Tok::Plain); // the newline itself
        assert_eq!(&t[7..14], &[Tok::Plain; 7], "next line: no state survives");
        // unterminated block comment runs to the line end / barrier
        let t = toks("x /* let fn");
        assert_eq!(&t[2..4], &[Tok::SpanOpen(1); 2]);
        assert_eq!(&t[5..11], &[Tok::SpanBody(1); 6]);
        // `*` alone is body, only `*/` closes
        let t = toks("/* a*b */");
        assert_eq!(t[5], Tok::SpanBody(1));
        assert_eq!(&t[7..9], &[Tok::SpanClose(1); 2]);
    }

    #[test]
    fn lex_numbers() {
        let t = toks("a = 42;");
        assert_eq!(&t[4..6], &[Tok::Number; 2]);
        let t = toks("x = 3.14;");
        assert_eq!(&t[4..8], &[Tok::Number; 4]);
        let t = toks(" 1.max(2)");
        assert_eq!(t[1], Tok::Number);
        assert_eq!(t[2], Tok::Plain, "dot without a following digit stops");
        assert_eq!(t[3], Tok::Plain, "max stays plain");
        assert_eq!(t[7], Tok::Number);
        let t = toks("a1 x42");
        assert_eq!(t[0], Tok::Plain, "digit inside identifier");
        assert_eq!(t[4], Tok::Plain, "digits after ident chars are no number");
        let t = toks(" 0x1F_ff");
        assert_eq!(&t[1..8], &[Tok::Number; 7]);
        let t = lex(&rust(), "\n42", &|_| true);
        assert_eq!(&t[1..3], &[Tok::Plain; 2], "number at line start has no anchor");
        let t = lex(&rust(), "a\n 42", &|_| true);
        assert_eq!(&t[3..5], &[Tok::Number; 2], "indented number colors");
    }

    #[test]
    fn lex_barrier_kills_states() {
        // an unmapped char (here: the emoji) breaks the shaping run: the
        // string dies there and a later quote opens a fresh one
        let t = lex(&rust(), "a\"b\u{1F680}c\"d\"", &|c| c != '\u{1F680}');
        assert_eq!(t[1], Tok::StrOpen(0));
        assert_eq!(t[2], Tok::StrBody(0));
        assert_eq!(t[3], Tok::Plain); // the barrier itself
        assert_eq!(t[4], Tok::Plain); // coloring died with the run break
        assert_eq!(t[5], Tok::StrOpen(0)); // fresh string
        assert_eq!(t[6], Tok::StrBody(0));
    }

    // ---- escapes --------------------------------------------------------

    #[test]
    fn lex_string_escapes_do_not_close() {
        // `\"` is body: the string survives past it and closes at the end
        let t = toks("a = \"x\\\"y\";");
        assert_eq!(t[4], Tok::StrOpen(0));
        assert_eq!(&t[5..9], &[Tok::StrBody(0); 4], "x, \\, \" and y are body");
        assert_eq!(t[9], Tok::StrClose(0), "the real closing quote closes");
        // `\\` is a body pair; the following quote still closes
        let t = toks("\"a\\\\\"");
        assert_eq!(t[0], Tok::StrOpen(0));
        assert_eq!(&t[1..4], &[Tok::StrBody(0); 3], "a and the escaped backslash");
        assert_eq!(t[4], Tok::StrClose(0));
        // escaped quote at the very start, then a real close
        let t = toks("\"\\\"\"");
        assert_eq!(t[0], Tok::StrOpen(0));
        assert_eq!(&t[1..3], &[Tok::StrBody(0); 2]);
        assert_eq!(t[3], Tok::StrClose(0));
        // dangling escape at the line end: pair cannot form, the lone
        // backslash is body and the state dies at the newline
        let t = toks("\"x\\\nlet y");
        assert_eq!(t[2], Tok::StrBody(0), "the lone backslash is body");
        assert_eq!(t[3], Tok::Plain, "the newline");
        assert_eq!(&t[4..7], &[Tok::Plain; 3]);
    }

    #[test]
    fn lex_escapes_in_every_quote_kind() {
        // js: single-quoted with \' and a template with \`
        let spec = javascript();
        let t = lex(&spec, "'it\\'s'", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(1));
        assert_eq!(&t[1..6], &[Tok::StrBody(1); 5]);
        assert_eq!(t[6], Tok::StrClose(1));
        let t = lex(&spec, "`a\\`b`", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(2));
        assert_eq!(&t[1..5], &[Tok::StrBody(2); 4], "escaped backtick is body");
        assert_eq!(t[5], Tok::StrClose(2));
        // python f-string with escapes behaves like any string
        let spec = python();
        let t = lex(&spec, "f\"a\\\"b\"", &|_| true);
        assert_eq!(t[1], Tok::StrOpen(0));
        assert_eq!(&t[2..6], &[Tok::StrBody(0); 4]);
        assert_eq!(t[6], Tok::StrClose(0));
    }

    // ---- JavaScript -----------------------------------------------------

    #[test]
    fn lex_js_quote_kinds_are_independent() {
        let t = lex(&javascript(), "'a\"b'", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(1), "single quote opens kind 1");
        assert_eq!(
            t[2],
            Tok::StrBody(1),
            "double quote inside single-quoted string is body"
        );
        assert_eq!(t[4], Tok::StrClose(1));
        let t = lex(&javascript(), "\"a'b\"", &|_| true);
        assert_eq!(
            t[2],
            Tok::StrBody(0),
            "single quote inside double-quoted string is body"
        );
        // `//` inside any string stays string
        let t = lex(&javascript(), "'a // b'", &|_| true);
        assert_eq!(&t[3..5], &[Tok::StrBody(1); 2]);
    }

    #[test]
    fn lex_js_template_literals_are_line_scoped() {
        // a multi-line template colors its opening line; the continuation
        // line is a fresh run and its closing backtick opens a new,
        // unterminated template
        let t = lex(&javascript(), "`a\nb`", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(2));
        assert_eq!(t[1], Tok::StrBody(2));
        assert_eq!(t[2], Tok::Plain); // newline kills the template
        assert_eq!(t[3], Tok::Plain);
        assert_eq!(t[4], Tok::StrOpen(2), "fresh template on the new line");
        // ordinary quotes die the same way
        let t = lex(&javascript(), "\"a\nb\"", &|_| true);
        assert_eq!(t[2], Tok::Plain, "double-quoted string dies at newline");
        assert_eq!(t[3], Tok::Plain);
        assert_eq!(t[4], Tok::StrOpen(0), "fresh string after the break");
        // interpolation degrades to string body (documented v1.2 scope)
        let t = lex(&javascript(), "`x ${y} z`", &|_| true);
        assert_eq!(&t[1..9], &[Tok::StrBody(2); 8]);
    }

    #[test]
    fn lex_js_keyword_groups_and_dollar() {
        let spec = javascript();
        let t = lex(&spec, "a return b", &|_| true);
        assert_eq!(&t[2..8], &[Tok::Keyword(0); 6], "control group");
        let t = lex(&spec, "a function b", &|_| true);
        assert_eq!(&t[2..10], &[Tok::Keyword(1); 8], "storage group");
        let t = lex(&spec, "a true b", &|_| true);
        assert_eq!(&t[2..6], &[Tok::Keyword(2); 4], "constant group");
        // '$' is an identifier char: $return is a name, not a keyword
        let t = lex(&spec, "a $return b", &|_| true);
        assert_eq!(&t[2..9], &[Tok::Plain; 7]);
        let t = lex(&spec, "a return$ b", &|_| true);
        assert_eq!(&t[2..9], &[Tok::Plain; 7]);
        // a keyword may anchor on '$' only when it ends a word — it does
        // not, because '$' is an identifier char
        let t = lex(&spec, "a$ return $b", &|_| true);
        assert_eq!(&t[3..9], &[Tok::Keyword(0); 6]);
    }

    // ---- Python ---------------------------------------------------------

    #[test]
    fn lex_python_triple_quotes_beat_simple() {
        let spec = python();
        // `"""` opens the block string, not an empty `""` pair
        let t = lex(&spec, "\"\"\"x\"\"\"", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(0); 3]);
        assert_eq!(t[3], Tok::SpanBody(0));
        assert_eq!(&t[4..7], &[Tok::SpanClose(0); 3]);
        // still `""` when no third quote follows
        let t = lex(&spec, "\"\"x", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(0));
        assert_eq!(t[1], Tok::StrClose(0));
        assert_eq!(t[2], Tok::Plain);
        // four quotes: triple open + one body char, unterminated
        let t = lex(&spec, "\"\"\"\"", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(0); 3]);
        assert_eq!(t[3], Tok::SpanBody(0));
        // empty triple string (six quotes)
        let t = lex(&spec, "\"\"\"\"\"\"", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(0); 3]);
        assert_eq!(&t[3..6], &[Tok::SpanClose(0); 3]);
        // lone quotes inside a triple string are body
        let t = lex(&spec, "\"\"\"a\"\"b\"\"\"", &|_| true);
        assert_eq!(t[4], Tok::SpanBody(0));
        assert_eq!(t[5], Tok::SpanBody(0));
        assert_eq!(&t[7..10], &[Tok::SpanClose(0); 3]);
    }

    #[test]
    fn lex_python_triple_kinds_and_lines() {
        let spec = python();
        // a multi-line docstring colors its opening line; the second line
        // is a fresh run — its trailing `"""` opens a new (unterminated)
        // triple rather than closing the first
        let t = lex(&spec, "\"\"\"Doc\nstring.\"\"\"", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(0); 3]);
        assert_eq!(&t[3..6], &[Tok::SpanBody(0); 3]); // Doc
        assert_eq!(t[6], Tok::Plain); // the newline
        assert_eq!(&t[7..14], &[Tok::Plain; 7]); // string.
        assert_eq!(&t[14..17], &[Tok::SpanOpen(0); 3]); // fresh, unterminated
        // single-quoted triple is a different kind
        let t = lex(&spec, "'''a\"\"\"b'''", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(1); 3]);
        assert_eq!(
            &t[4..7],
            &[Tok::SpanBody(1); 3],
            "`\"\"\"` inside `'''` is body"
        );
        assert_eq!(&t[8..11], &[Tok::SpanClose(1); 3]);
        // simple strings die at the newline
        let t = lex(&spec, "\"a\nb\"", &|_| true);
        assert_eq!(t[2], Tok::Plain);
        // f-string prefix stays plain, the string still colors
        let t = lex(&spec, "f\"x{y}\"", &|_| true);
        assert_eq!(t[0], Tok::Plain);
        assert_eq!(t[1], Tok::StrOpen(0));
        assert_eq!(&t[2..6], &[Tok::StrBody(0); 4]);
        assert_eq!(t[6], Tok::StrClose(0));
    }

    #[test]
    fn lex_python_hash_comments() {
        let spec = python();
        let t = lex(&spec, "# \"q\" x\ndef y():\n    pass\n", &|_| true);
        assert_eq!(t[0], Tok::SpanOpen(2));
        assert_eq!(&t[1..7], &[Tok::SpanBody(2); 6]);
        assert_eq!(t[7], Tok::Plain, "newline ends the comment");
        assert_eq!(&t[8..11], &[Tok::Plain; 3], "def at line start: no anchor");
        assert_eq!(
            &t[21..25],
            &[Tok::Plain; 4],
            "keyword at line end: no lookahead glyph in its run"
        );
        // indented mid-line keywords color normally
        let t = lex(&spec, "z = 1\n    if x and not y:\n        pass\n", &|_| true);
        assert_eq!(&t[10..12], &[Tok::Keyword(0); 2], "if after indentation");
        assert_eq!(&t[15..18], &[Tok::Keyword(3); 3], "and is group 3");
    }
}
