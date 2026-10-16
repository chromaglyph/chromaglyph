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
//!   boundaries via the "not an identifier char" class (plus the newline
//!   class, so keywords at a line start color).
//! - **line comments**: an opening rule substitutes the `//` into the
//!   comment state; *propagation* rules (backtrack = state class, input =
//!   one class per glyph partition) carry the color glyph by glyph; the
//!   state ends at the newline, which has no line-comment copy.
//! - **block comments**: string-like open/close rules around `/*` `*/`,
//!   and the state *spans* newlines.
//! - **strings**: the opening quote enters the string state; the closing
//!   quote substitutes into a distinct *end* class so propagation stops
//!   right after it. (Open/close get separate identically-colored slots
//!   because a nested lookup maps a glyph to exactly one substitute.)
//! - **numbers**: a digit after a word boundary opens a number state that
//!   propagates over identifier classes; `.` continues the state only
//!   when a digit follows (`3.14` colors, `1.max` does not swallow `max`).
//!
//! A newline glyph is **injected** into the output font (empty, zero
//! width, plain — see `plain_glyphs`) so shaping runs do not break at
//! line ends in browsers: keyword boundaries can anchor on it and block
//! comments survive it. Unmapped chars (emoji, …) still break runs —
//! states die there, in browsers and in the oracle alike.
//!
//! Two mechanisms make the states work under real shapers (which apply a
//! contextual lookup left to right, seeing earlier substitutions in the
//! same pass):
//!
//! 1. **Rule order within each first-glyph rule set**: state propagation
//!    before state terminators before state openings before keywords, so
//!    `//` inside a string stays a string and `fn` inside a comment stays
//!    a comment.
//! 2. **Exclusive state classes**: propagation backtracks match only
//!    colored copies, never plain glyphs, so a state always starts at its
//!    delimiter and always ends at its terminator.
//!
//! Everything stays slot-based: one CPAL palette per theme colors the
//! same rule set under every theme; `@font-palette-values` flips themes
//! with no re-shaping.
//!
//! v1.1 scope: keywords, `//` and `/* */` comments, one non-nested
//! double-quoted string (no escapes, per-language newline spanning),
//! numbers; a single language per font.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::compile::{GlyphPlan, Rule};
use crate::error::BuildError;
use crate::spec::Color;
use crate::theme::ThemeSpec;
use crate::verify::VerifyResult;

/// What a language looks like to the rule synthesizer. Colors come from
/// the themes by resolving `*_scope` like a syntect highlighter would.
pub struct LanguageSpec {
    pub name: &'static str,
    pub keywords: &'static [&'static str],
    /// Line comment opener, e.g. `"//"`; runs to the next `\n`.
    pub line_comment: Option<&'static str>,
    /// Block comment delimiters, e.g. `Some(("/*", "*/"))`; may span
    /// lines (the injected newline glyph keeps the run intact).
    pub block_comment: Option<(&'static str, &'static str)>,
    /// String delimiter, e.g. `'"'`; runs to the next occurrence.
    pub string_quote: Option<char>,
    /// Color `42`, `3.14`, `0x1F` … as numbers.
    pub numbers: bool,
    /// Whether string literals may span newlines (comments always may).
    pub strings_span_newlines: bool,
    pub keyword_scope: &'static str,
    pub string_scope: &'static str,
    pub comment_scope: &'static str,
    pub number_scope: &'static str,
}

/// Built-in Rust spec: keywords per the reference (2018 edition) grammar.
pub fn rust() -> LanguageSpec {
    LanguageSpec {
        name: "rust",
        keywords: &[
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
            "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait",
            "true", "type", "unsafe", "use", "where", "while",
        ],
        line_comment: Some("//"),
        block_comment: Some(("/*", "*/")),
        string_quote: Some('"'),
        numbers: true,
        strings_span_newlines: false,
        keyword_scope: "keyword.other.rust",
        string_scope: "string.quoted.double.rust",
        comment_scope: "comment.line.double-slash.rust",
        number_scope: "constant.numeric.rust",
    }
}

/// Resolve a language spec by name (the `--generic` CLI argument).
pub fn spec_by_name(name: &str) -> Result<LanguageSpec, BuildError> {
    match name {
        "rust" | "rs" => Ok(rust()),
        other => Err(BuildError::Tokenize(format!(
            "unknown generic language {other:?}; available: rust"
        ))),
    }
}

// ---- slot layout ---------------------------------------------------------
// Slots are palette entries AND nested-lookup indices (see gsub). Opening
// and closing delimiters use distinct slots because a nested single-subst
// lookup maps a glyph to exactly one substitute, but both paint the same
// color. Body states need a copy of (nearly) every glyph.
const KW: u16 = 0; // keyword letters
const S_OPEN: u16 = 1; // opening quote
const S_BODY: u16 = 2; // string body glyphs
const S_CLOSE: u16 = 3; // closing quote
const C_OPEN: u16 = 4; // line-comment slashes
const C_BODY: u16 = 5; // line-comment body glyphs
const NUM: u16 = 6; // number glyphs (digits, letters, dot)
const BC_OPEN: u16 = 7; // block-comment `/*`
const BC_BODY: u16 = 8; // block-comment body glyphs
const BC_CLOSE: u16 = 9; // block-comment `*/`

fn is_ident(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

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
    // ---- per-theme colors for the token kinds ----------------------------
    let kw_colors: Vec<Option<Color>> = themes
        .iter()
        .map(|t| scope_color(t, spec.keyword_scope))
        .collect();
    let str_colors: Vec<Option<Color>> = themes
        .iter()
        .map(|t| scope_color(t, spec.string_scope))
        .collect();
    let cmt_colors: Vec<Option<Color>> = themes
        .iter()
        .map(|t| scope_color(t, spec.comment_scope))
        .collect();
    let num_colors: Vec<Option<Color>> = themes
        .iter()
        .map(|t| scope_color(t, spec.number_scope))
        .collect();
    let slots: Vec<Vec<Option<Color>>> = vec![
        kw_colors,
        str_colors.clone(),
        str_colors.clone(),
        str_colors,
        cmt_colors.clone(),
        cmt_colors.clone(),
        num_colors,
        cmt_colors.clone(),
        cmt_colors.clone(),
        cmt_colors,
    ];
    let theme_fgs: Vec<Color> = themes.iter().map(|t| t.foreground).collect();

    // ---- glyph classes ----------------------------------------------------
    // Every mapped glyph is partitioned by the role of its character:
    // delimiters with rules of their own (quote, slash, star, dot, newline)
    // get 1:1 classes, keyword letters get 1:1 classes (keyword inputs),
    // digits get one class (number inputs), other identifier chars share a
    // class, and the rest (including .notdef) is the "not a word char"
    // class used for keyword boundaries.
    let quote = spec.string_quote;
    let line_comment: Vec<char> = spec
        .line_comment
        .map(|s| s.chars().collect())
        .unwrap_or_default();
    let comment_first = line_comment.first().copied();
    let (bc_open, bc_close): (Vec<char>, Vec<char>) = spec
        .block_comment
        .map(|(o, c)| {
            (
                o.chars().collect::<Vec<_>>(),
                c.chars().collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();
    let bc_chars: HashSet<char> = bc_open.iter().chain(&bc_close).copied().collect();
    let keyword_letters: Vec<char> = {
        let mut set: Vec<char> = spec
            .keywords
            .iter()
            .flat_map(|k| k.chars())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        set.sort_unstable();
        set
    };

    let c_notword: u16 = 1;
    let c_ident: u16 = 2;
    let c_quote: u16 = 3;
    let c_slash: u16 = 4;
    let c_nl: u16 = 5;
    // .notdef is its own class, referenced by no rule: an unmapped char in
    // the oracle (and a font-fallback run break in browsers) must NOT count
    // as a word boundary a keyword could anchor on
    let c_notdef: u16 = 6;
    let c_digit: u16 = 7;
    let c_dot: u16 = 8;
    let c_star: u16 = 9;
    let first_letter_class: u16 = 10;
    let letter_class: HashMap<char, u16> = keyword_letters
        .iter()
        .enumerate()
        .map(|(i, &c)| (c, first_letter_class + i as u16))
        .collect();
    // state classes (exclusive sets of colored copies)
    let c_str_state: u16 = first_letter_class + keyword_letters.len() as u16;
    let c_cmt_state: u16 = c_str_state + 1;
    let c_bcmt_state: u16 = c_cmt_state + 1;
    let c_num_state: u16 = c_bcmt_state + 1;
    let c_end_state: u16 = c_num_state + 1;

    let mut class_of: HashMap<u16, u16> = HashMap::new();
    class_of.insert(0, c_notdef); // .notdef: never a context, never substituted
    let mut sorted = mappings.to_vec();
    sorted.sort_unstable();
    let mut newline_gid: Option<u16> = None;
    for &(cp, gid) in &sorted {
        let Some(ch) = char::from_u32(cp) else { continue };
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
        let class = if Some(ch) == quote {
            c_quote
        } else if ch == '*' {
            c_star // block-comment close partner, rules target it directly
        } else if Some(ch) == comment_first || bc_chars.contains(&ch) {
            c_slash // '/' opens/closes both comment kinds
        } else if ch == '.' {
            c_dot
        } else if ch.is_ascii_digit() {
            c_digit
        } else if is_ident(ch) {
            letter_class.get(&ch).copied().unwrap_or(c_ident)
        } else {
            c_notword
        };
        class_of.insert(gid, class);
    }
    let quote_gid = quote.and_then(|q| char_gid(mappings, q));
    let slash_gid = comment_first
        .or_else(|| bc_open.first().copied())
        .and_then(|c| char_gid(mappings, c));
    let star_gid = char_gid(mappings, '*');
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
    // keyword letters (keyword copies share the base class so keyword
    // rules stay form-insensitive)
    for &c in &keyword_letters {
        if let Some(gid) = char_gid(mappings, c) {
            let class = class_of[&gid];
            push_alt(&mut next_new, &mut alt_glyphs, gid, KW, class, &mut class_of);
        }
    }
    // number glyphs: digits, identifier chars, letters, dot
    let num_glyphs: Vec<u16> = gids
        .iter()
        .copied()
        .filter(|&g| class_of[&g] != c_notword && class_of[&g] != c_nl)
        .filter(|&g| Some(g) != quote_gid)
        .collect();
    // string delimiters
    if let Some(q) = quote_gid {
        push_alt(&mut next_new, &mut alt_glyphs, q, S_OPEN, c_str_state, &mut class_of);
        push_alt(&mut next_new, &mut alt_glyphs, q, S_CLOSE, c_end_state, &mut class_of);
    }
    // line comment opener
    if let Some(s) = slash_gid {
        if line_comment.len() >= 2 {
            push_alt(&mut next_new, &mut alt_glyphs, s, C_OPEN, c_cmt_state, &mut class_of);
        }
    }
    // block comment delimiters
    if !bc_open.is_empty() {
        if let Some(s) = slash_gid {
            push_alt(&mut next_new, &mut alt_glyphs, s, BC_OPEN, c_bcmt_state, &mut class_of);
        }
        if let Some(st) = star_gid {
            push_alt(&mut next_new, &mut alt_glyphs, st, BC_OPEN, c_bcmt_state, &mut class_of);
        }
        if let Some(st) = star_gid {
            push_alt(&mut next_new, &mut alt_glyphs, st, BC_CLOSE, c_end_state, &mut class_of);
        }
        if let Some(s) = slash_gid {
            push_alt(&mut next_new, &mut alt_glyphs, s, BC_CLOSE, c_end_state, &mut class_of);
        }
    }
    if spec.numbers && digit_gid.is_some() {
        for &g in &num_glyphs {
            push_alt(&mut next_new, &mut alt_glyphs, g, NUM, c_num_state, &mut class_of);
        }
    }
    // string body: everything except the quote (the close rule owns it)
    if quote_gid.is_some() {
        for &gid in &gids {
            if Some(gid) != quote_gid {
                push_alt(&mut next_new, &mut alt_glyphs, gid, S_BODY, c_str_state, &mut class_of);
            }
        }
    }
    // line-comment body: everything except the newline (slashes and stars
    // inside a comment are body — coverage misses would break the chain)
    if line_comment.len() >= 2 {
        for &gid in &gids {
            if class_of[&gid] != c_nl {
                push_alt(&mut next_new, &mut alt_glyphs, gid, C_BODY, c_cmt_state, &mut class_of);
            }
        }
    }
    // block-comment body: everything (newlines included — the state spans)
    if !bc_open.is_empty() {
        for &gid in &gids {
            push_alt(&mut next_new, &mut alt_glyphs, gid, BC_BODY, c_bcmt_state, &mut class_of);
        }
    }

    // ---- injected newline glyph -------------------------------------------
    // The source font almost never maps '\n'; without a glyph the browser
    // sends it to a fallback font, breaking the shaping run — killing
    // keyword boundaries at line starts and any state that should span
    // lines. An empty zero-width glyph mapped to \n and \r keeps runs
    // whole. It is plain (uncolored), class c_nl: no line-comment or
    // number propagation references that class, so line comments still
    // end at newlines and numbers do not span them.
    let mut plain_glyphs: Vec<(u32, u16)> = Vec::new();
    if newline_gid.is_none() {
        let gid = next_new;
        next_new += 1;
        class_of.insert(gid, c_nl);
        plain_glyphs.push(('\n' as u32, gid));
        plain_glyphs.push(('\r' as u32, gid));
        // block comments span newlines, so the glyph needs a body copy
        if !bc_open.is_empty() {
            push_alt(&mut next_new, &mut alt_glyphs, gid, BC_BODY, c_bcmt_state, &mut class_of);
        }
    }

    // ---- rules ------------------------------------------------------------
    // Order inside a first-glyph rule set decides who wins at a position:
    // state propagation, state terminators, state openings, keywords.
    let mut by_class: BTreeMap<u16, Vec<Rule>> = BTreeMap::new();
    fn prop(by_class: &mut BTreeMap<u16, Vec<Rule>>, class: u16, state: u16, pal: u16, lookahead: Vec<u16>) {
        by_class.entry(class).or_default().push(Rule {
            backtrack: vec![state],
            input: vec![class],
            lookahead,
            pal,
        });
    }
    let present: Vec<u16> = {
        let mut v: Vec<u16> = class_of.values().copied().collect::<HashSet<_>>().into_iter().collect();
        v.sort_unstable();
        v
    };
    // body class sets per state
    let str_body = |class: u16| -> bool {
        quote_gid.is_some() && class != c_quote && class != c_notdef &&
            (spec.strings_span_newlines || class != c_nl)
    };
    let line_cmt_body = |class: u16| -> bool {
        line_comment.len() >= 2 && class != c_nl && class != c_notdef
    };
    let block_cmt_body = |class: u16| -> bool { !bc_open.is_empty() && class != c_notdef };
    let num_body = |class: u16| -> bool {
        spec.numbers
            && digit_gid.is_some()
            && class != c_notword
            && class != c_nl
            && class != c_notdef
            && class != c_quote
    };
    // keyword boundary classes: plain punctuation or the (now mapped)
    // newline — both exist as glyphs the shaper can anchor on
    let boundaries = [c_notword, c_nl];

    for &class in &present {
        if class == c_quote {
            if quote_gid.is_some() {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![c_str_state],
                    input: vec![c_quote],
                    lookahead: vec![],
                    pal: S_CLOSE,
                });
            }
            if block_cmt_body(c_quote) {
                prop(&mut by_class, c_quote, c_bcmt_state, BC_BODY, vec![]);
            }
            if line_cmt_body(c_quote) {
                prop(&mut by_class, c_quote, c_cmt_state, C_BODY, vec![]);
            }
            if quote_gid.is_some() {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![],
                    input: vec![c_quote],
                    lookahead: vec![],
                    pal: S_OPEN,
                });
            }
        } else if class == c_slash {
            if str_body(c_slash) {
                prop(&mut by_class, c_slash, c_str_state, S_BODY, vec![]);
            }
            if block_cmt_body(c_slash) {
                prop(&mut by_class, c_slash, c_bcmt_state, BC_BODY, vec![]);
            }
            if line_cmt_body(c_slash) {
                prop(&mut by_class, c_slash, c_cmt_state, C_BODY, vec![]);
            }
            let rules = by_class.entry(class).or_default();
            if line_comment.len() >= 2 {
                rules.push(Rule {
                    backtrack: vec![],
                    input: vec![c_slash, c_slash],
                    lookahead: vec![],
                    pal: C_OPEN,
                });
            }
            if !bc_open.is_empty() && star_gid.is_some() {
                rules.push(Rule {
                    backtrack: vec![],
                    input: vec![c_slash, c_star],
                    lookahead: vec![],
                    pal: BC_OPEN,
                });
            }
        } else if class == c_star && !bc_open.is_empty() {
            // close: `*/` returns to plain text
            if star_gid.is_some() {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![c_bcmt_state],
                    input: vec![c_star, c_slash],
                    lookahead: vec![],
                    pal: BC_CLOSE,
                });
            }
            if block_cmt_body(c_star) {
                prop(&mut by_class, c_star, c_bcmt_state, BC_BODY, vec![]);
            }
            if str_body(c_star) {
                prop(&mut by_class, c_star, c_str_state, S_BODY, vec![]);
            }
            if line_cmt_body(c_star) {
                prop(&mut by_class, c_star, c_cmt_state, C_BODY, vec![]);
            }
        } else if class == c_dot && spec.numbers {
            if str_body(c_dot) {
                prop(&mut by_class, c_dot, c_str_state, S_BODY, vec![]);
            }
            if block_cmt_body(c_dot) {
                prop(&mut by_class, c_dot, c_bcmt_state, BC_BODY, vec![]);
            }
            if line_cmt_body(c_dot) {
                prop(&mut by_class, c_dot, c_cmt_state, C_BODY, vec![]);
            }
            // `.` continues a number only when a digit follows: 3.14 yes,
            // 1.max stops before the dot
            prop(&mut by_class, c_dot, c_num_state, NUM, vec![c_digit]);
        } else if class == c_digit && spec.numbers {
            if str_body(c_digit) {
                prop(&mut by_class, c_digit, c_str_state, S_BODY, vec![]);
            }
            if block_cmt_body(c_digit) {
                prop(&mut by_class, c_digit, c_bcmt_state, BC_BODY, vec![]);
            }
            if line_cmt_body(c_digit) {
                prop(&mut by_class, c_digit, c_cmt_state, C_BODY, vec![]);
            }
            if num_body(c_digit) {
                prop(&mut by_class, c_digit, c_num_state, NUM, vec![]);
            }
            // a number opens after a word boundary (or at a line start)
            for &bt in &boundaries {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![bt],
                    input: vec![c_digit],
                    lookahead: vec![],
                    pal: NUM,
                });
            }
        } else {
            if str_body(class) {
                prop(&mut by_class, class, c_str_state, S_BODY, vec![]);
            }
            if block_cmt_body(class) {
                prop(&mut by_class, class, c_bcmt_state, BC_BODY, vec![]);
            }
            if line_cmt_body(class) {
                prop(&mut by_class, class, c_cmt_state, C_BODY, vec![]);
            }
            if num_body(class) {
                prop(&mut by_class, class, c_num_state, NUM, vec![]);
            }
        }
    }
    // keywords, attached to the rule set of their first letter's class:
    // four boundary combinations (plain punctuation / newline on each side)
    for kw in spec.keywords {
        let Some(first) = kw.chars().next() else { continue };
        let Some(&class) = letter_class.get(&first) else {
            continue;
        };
        let input: Vec<u16> = kw.chars().map(|c| letter_class[&c]).collect();
        for &bt in &boundaries {
            for &la in &boundaries {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![bt],
                    input: input.clone(),
                    lookahead: vec![la],
                    pal: KW,
                });
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

/// Resolve a scope's foreground in a theme the way the exact-mode
/// pipeline would (syntect's selector engine over the theme).
fn scope_color(theme: &ThemeSpec, scope: &str) -> Option<Color> {
    let highlighter = syntect::highlighting::Highlighter::new(&theme.theme);
    let mut stack = syntect::parsing::ScopeStack::new();
    // one dotted scope = one stack atom; selectors match it hierarchically
    stack.push(syntect::parsing::Scope::new(scope).ok()?);
    let modifier = highlighter.style_mod_for_stack(&stack.scopes);
    modifier.foreground.map(|c| Color {
        red: c.r,
        green: c.g,
        blue: c.b,
        alpha: c.a,
    })
}

// ---- reference lexer + oracle ---------------------------------------------

/// Reference lexer output per character; the font must reproduce exactly
/// this when shaped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Number,
    StrOpen,
    StrBody,
    StrClose,
    CmtOpen,
    CmtBody,
    BCmtOpen,
    BCmtBody,
    BCmtClose,
}

fn slot_of(tok: Tok) -> Option<u16> {
    Some(match tok {
        Tok::Plain => return None,
        Tok::Keyword => KW,
        Tok::Number => NUM,
        Tok::StrOpen => S_OPEN,
        Tok::StrBody => S_BODY,
        Tok::StrClose => S_CLOSE,
        Tok::CmtOpen => C_OPEN,
        Tok::CmtBody => C_BODY,
        Tok::BCmtOpen => BC_OPEN,
        Tok::BCmtBody => BC_BODY,
        Tok::BCmtClose => BC_CLOSE,
    })
}

/// Highlight `text` per the spec, one [`Tok`] per char.
///
/// `mapped` says whether the *output* font has a glyph for a char; it
/// models the shaper's real capabilities. States die at unmapped chars
/// (browsers send those to a fallback font, breaking the shaping run).
/// Keywords and numbers open only after a mapped plain-punctuation or
/// newline glyph; string literals stop at newlines unless
/// `strings_span_newlines` (the font mirrors this exactly).
pub fn lex(spec: &LanguageSpec, text: &str, mapped: &dyn Fn(char) -> bool) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = vec![Tok::Plain; chars.len()];
    // a word boundary glyph: mapped, not an identifier char, and not one
    // of the delimiter chars that live in their own classes
    let boundary = |c: char| {
        mapped(c)
            && !is_ident(c)
            && Some(c) != spec.string_quote
            && Some(c) != spec.line_comment.and_then(|s| s.chars().next())
            && c != '.'
            && c != '*'
    };
    let mut i = 0;
    while i < chars.len() {
        if let Some(comment) = spec.line_comment {
            let cs: Vec<char> = comment.chars().collect();
            if chars[i..].starts_with(&cs) {
                for tok in out.iter_mut().take(i + cs.len()).skip(i) {
                    *tok = Tok::CmtOpen;
                }
                i += cs.len();
                while i < chars.len() {
                    let c = chars[i];
                    if c == '\n' || c == '\r' || !mapped(c) {
                        break;
                    }
                    out[i] = Tok::CmtBody;
                    i += 1;
                }
                continue;
            }
        }
        if let Some((open, close)) = spec.block_comment {
            let open: Vec<char> = open.chars().collect();
            let close: Vec<char> = close.chars().collect();
            if chars[i..].starts_with(&open) {
                let n = open.len();
                for tok in out.iter_mut().take(i + n).skip(i) {
                    *tok = Tok::BCmtOpen;
                }
                i += n;
                while i < chars.len() {
                    if chars[i..].starts_with(&close) {
                        for k in 0..close.len() {
                            out[i + k] = Tok::BCmtClose;
                        }
                        i += close.len();
                        break;
                    }
                    if !mapped(chars[i]) {
                        break; // barrier: state dies, plain scan resumes
                    }
                    out[i] = Tok::BCmtBody;
                    i += 1;
                }
                continue;
            }
        }
        if let Some(q) = spec.string_quote {
            if chars[i] == q {
                out[i] = Tok::StrOpen;
                i += 1;
                while i < chars.len() {
                    if chars[i] == q {
                        out[i] = Tok::StrClose;
                        i += 1;
                        break;
                    }
                    let c = chars[i];
                    if !mapped(c) || (!spec.strings_span_newlines && (c == '\n' || c == '\r')) {
                        break; // barrier or line end: state dies
                    }
                    out[i] = Tok::StrBody;
                    i += 1;
                }
                continue;
            }
        }
        if spec.numbers && chars[i].is_ascii_digit() {
            let start = i;
            if start > 0 && boundary(chars[start - 1]) {
                while i < chars.len() && is_ident(chars[i]) {
                    out[i] = Tok::Number;
                    i += 1;
                }
                // fractional part: a dot continues the number only when
                // a digit follows it (mirrors the font's lookahead rule)
                while i < chars.len() {
                    let continues = is_ident(chars[i])
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
        if is_ident(chars[i]) {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if spec.keywords.contains(&word.as_str())
                && start > 0
                && boundary(chars[start - 1])
                && i < chars.len()
                && boundary(chars[i])
            {
                for tok in out.iter_mut().take(i).skip(start) {
                    *tok = Tok::Keyword;
                }
            }
            continue;
        }
        i += 1;
    }
    out
}

/// Curated plus deterministic-random texts; the oracle must hold on all.
pub fn sample_texts(_spec: &LanguageSpec) -> Vec<String> {
    let mut samples = vec![
        "let x = 1;".to_string(),
        "// comment\nfn main() {}".to_string(),
        "let s = \"fn // let\";".to_string(),
        "// \"quoted\" comment\nlet y;".to_string(),
        "\"\"\"".to_string(),
        "//////".to_string(),
        "fnfn fn fnx xfn _fn fn_".to_string(),
        "a\"b\"c\"d".to_string(),
        "/\n// x\n/".to_string(),
        "let s = \"\";".to_string(),
        "struct S; impl S { fn f() -> Self { Self } }".to_string(),
        "/* block */ let x;".to_string(),
        "/* multi\nline\ncomment */ let y;".to_string(),
        "/* unterminated block let fn".to_string(),
        "a/*b\"c*/d\"e".to_string(),
        "*/*/*/**/".to_string(),
        "let a = 42; let b = 3.14; let c = 0x1F_ff_u32; let d = 1_000;".to_string(),
        "1.max(2).min(3)".to_string(),
        "x1 12x a1b2 0b1010 1e5 1.2.3 42.".to_string(),
        "\nfn at_line_start\nlet at_line_start\n42 at_line_start".to_string(),
    ];
    let pieces = [
        "fn", "let", "\"ab\"", "\"fn\"", "// c\n", "/", "x", "_y1", " ", "=", ";", "\"", "//",
        "\"str // inner\"", "mut", "pub", "\n", "s", "use", "42", "3.14", "0x1F", "/* b */",
        "/*", "*/", "1.max", ".", "*", "2",
    ];
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
            .map(|_| pieces[next() as usize % pieces.len()])
            .collect();
        samples.push(s);
    }
    samples
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
                let want = match slot_of(tok) {
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
        // the built font injects a newline glyph, so everything maps
        lex(&rust(), s, &|_| true)
    }

    #[test]
    fn lex_keywords_and_boundaries() {
        // keyword between punctuation colors; at text start it cannot
        // (no glyph exists for the shaper to backtrack on)
        let t = toks("a let b");
        assert_eq!(&t[2..5], &[Tok::Keyword; 3]);
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
    fn lex_line_start_keywords_with_newline_mapping() {
        // with '\n' mapped (the injected glyph), a keyword after a newline
        // anchors on it and colors
        let t = lex(&rust(), "a\nfn b", &|_| true);
        assert_eq!(&t[2..4], &[Tok::Keyword; 2]);
    }

    #[test]
    fn lex_strings_and_comments() {
        let t = toks("a \"b // c\" d");
        assert_eq!(t[2], Tok::StrOpen);
        assert_eq!(&t[3..9], &[Tok::StrBody; 6]); // `//` inside a string stays string
        assert_eq!(t[9], Tok::StrClose);
        let t = toks("// \"q\" x\nfn y");
        assert_eq!(&t[0..2], &[Tok::CmtOpen; 2]);
        assert_eq!(&t[2..8], &[Tok::CmtBody; 6]); // quote inside a comment stays comment
        assert_eq!(t[8], Tok::Plain); // newline ends the comment
        assert_eq!(
            &t[9..11],
            &[Tok::Keyword; 2],
            "the injected newline glyph anchors line-start keywords"
        );
    }

    #[test]
    fn lex_block_comments() {
        let t = toks("a /* b\n c */ d");
        assert_eq!(&t[2..4], &[Tok::BCmtOpen; 2]);
        assert_eq!(t[8], Tok::BCmtBody, "block comments span newlines");
        assert_eq!(&t[10..12], &[Tok::BCmtClose; 2]);
        // unterminated block comment runs to the barrier/end
        let t = toks("x /* let fn");
        assert_eq!(&t[2..4], &[Tok::BCmtOpen; 2]);
        assert_eq!(&t[5..11], &[Tok::BCmtBody; 6]);
        // `*` alone is body, only `*/` closes
        let t = toks("/* a*b */");
        assert_eq!(t[5], Tok::BCmtBody);
        assert_eq!(&t[7..9], &[Tok::BCmtClose; 2]);
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
        assert_eq!(&t[1..3], &[Tok::Number; 2], "number at line start");
    }

    #[test]
    fn lex_barrier_kills_states() {
        // an unmapped char (here: the emoji) breaks the shaping run: the
        // string dies there and a later quote opens a fresh one
        let t = lex(
            &rust(),
            "a\"b\u{1F680}c\"d\"",
            &|c| c != '\u{1F680}',
        );
        assert_eq!(t[1], Tok::StrOpen);
        assert_eq!(t[2], Tok::StrBody);
        assert_eq!(t[3], Tok::Plain); // the barrier itself
        assert_eq!(t[4], Tok::Plain); // coloring died with the run break
        assert_eq!(t[5], Tok::StrOpen); // fresh string
        assert_eq!(t[6], Tok::StrBody);
    }
}
