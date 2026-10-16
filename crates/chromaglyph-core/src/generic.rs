//! Generic mode: highlight **any** text of a language, content unknown at
//! build time.
//!
//! Exact mode compiles known text into one rule per occurrence. Generic
//! mode instead synthesizes a tiny state machine out of contextual
//! substitution rules — the idea Cobble / FontWithASyntaxHighlighter
//! proved by hand (706 rules for one language), here generated from a
//! language spec:
//!
//! - **keywords**: one rule per keyword, letters as 1:1 classes, word
//!   boundaries via a shared "not an identifier char" class on both sides.
//! - **line comments**: an opening rule substitutes the `//` into the
//!   comment state; *propagation* rules (backtrack = comment-state class,
//!   input = one class per glyph partition) carry the color glyph by
//!   glyph; the state ends at the newline, which has no comment copy, so
//!   substitution coverage misses and propagation dies out — in browsers
//!   the font-fallback run break does the same job.
//! - **strings**: the opening quote enters the string state; the closing
//!   quote is substituted into a distinct *string-end* class so
//!   propagation stops right after it. Opening and closing quotes get
//!   separate (identically colored) slots because a nested lookup maps a
//!   glyph to exactly one substitute.
//!
//! Two mechanisms make the states work under real shapers (which apply a
//! contextual lookup left to right, seeing earlier substitutions in the
//! same pass):
//!
//! 1. **Rule order within each first-glyph rule set**: state propagation
//!    before state opening, so `//` inside a string stays a string and
//!    `fn` inside a comment stays a comment.
//! 2. **Exclusive state classes**: propagation backtracks match only
//!    colored copies, never plain glyphs, so a state always starts at its
//!    delimiter and always ends at its terminator.
//!
//! Everything stays slot-based: one CPAL palette per theme colors the
//! same rule set under every theme; `@font-palette-values` flips themes
//! with no re-shaping.
//!
//! v1 scope: keywords, `//` line comments, one non-nested double-quoted
//! string per line-ish span (no escapes), a single language per font.

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
    /// String delimiter, e.g. `'"'`; runs to the next occurrence.
    pub string_quote: Option<char>,
    pub keyword_scope: &'static str,
    pub string_scope: &'static str,
    pub comment_scope: &'static str,
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
        string_quote: Some('"'),
        keyword_scope: "keyword.other.rust",
        string_scope: "string.quoted.double.rust",
        comment_scope: "comment.line.double-slash.rust",
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
// and closing quotes use distinct slots because a nested single-subst
// lookup maps a glyph to exactly one substitute, but both must paint the
// string color.
const KW: u16 = 0; // keyword letters
const S_OPEN: u16 = 1; // opening quote
const S_BODY: u16 = 2; // string body glyphs
const S_CLOSE: u16 = 3; // closing quote
const C_OPEN: u16 = 4; // comment slashes
const C_BODY: u16 = 5; // comment body glyphs

fn is_ident(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Synthesize a plan for arbitrary text of `spec`.
///
/// `mappings` is the source font's cmap as (codepoint, glyph id) pairs;
/// `num_source_glyphs` is where alt glyph ids start.
pub fn synthesize(
    spec: &LanguageSpec,
    mappings: &[(u32, u16)],
    num_source_glyphs: u16,
    themes: &[ThemeSpec],
) -> Result<GlyphPlan, BuildError> {
    // ---- per-theme colors for the three token kinds ----------------------
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
    let slots: Vec<Vec<Option<Color>>> = vec![
        kw_colors,
        str_colors.clone(),
        str_colors.clone(),
        str_colors,
        cmt_colors.clone(),
        cmt_colors,
    ];
    let theme_fgs: Vec<Color> = themes.iter().map(|t| t.foreground).collect();

    // ---- glyph classes ----------------------------------------------------
    // Partition every mapped glyph by the role of its character:
    //  - the quote and the comment's first char get 1:1 classes (rules
    //    target them specifically),
    //  - newline gets its own class (comments must not cross it when the
    //    font happens to map it),
    //  - keyword letters get 1:1 classes (keyword inputs),
    //  - other identifier chars share one class,
    //  - everything else (including .notdef) shares the "not a word
    //    char" class used for keyword boundaries.
    let quote = spec.string_quote;
    let comment_chars: Vec<char> = spec
        .line_comment
        .map(|s| s.chars().collect())
        .unwrap_or_default();
    let comment_first = comment_chars.first().copied();
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
    let first_letter_class: u16 = 7;
    let letter_class: HashMap<char, u16> = keyword_letters
        .iter()
        .enumerate()
        .map(|(i, &c)| (c, first_letter_class + i as u16))
        .collect();
    let c_str_state: u16 = first_letter_class + keyword_letters.len() as u16;
    let c_cmt_state: u16 = c_str_state + 1;
    let c_end_state: u16 = c_cmt_state + 1;

    let mut class_of: HashMap<u16, u16> = HashMap::new();
    class_of.insert(0, c_notdef); // .notdef: never a context, never substituted
    let mut sorted = mappings.to_vec();
    sorted.sort_unstable();
    for &(cp, gid) in &sorted {
        let Some(ch) = char::from_u32(cp) else { continue };
        if class_of.contains_key(&gid) {
            continue; // first mapping of a glyph decides its class
        }
        let class = if Some(ch) == quote {
            c_quote
        } else if Some(ch) == comment_first {
            c_slash
        } else if ch == '\n' || ch == '\r' {
            c_nl
        } else if is_ident(ch) {
            letter_class.get(&ch).copied().unwrap_or(c_ident)
        } else {
            c_notword
        };
        class_of.insert(gid, class);
    }
    let quote_gid = quote.and_then(|q| char_gid(mappings, q));
    let slash_gid = comment_first.and_then(|c| char_gid(mappings, c));

    // ---- alt glyphs -------------------------------------------------------
    // Body states need a copy of (almost) every glyph: anything can appear
    // inside a string or comment. Empty copies, so the size cost is a few
    // bytes each (loca + hmtx + COLR rows).
    let mut gids: Vec<u16> = class_of
        .keys()
        .copied()
        .filter(|&g| g != 0)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    gids.sort_unstable();

    let mut alt_glyphs: Vec<(u16, u16, u16)> = Vec::new();
    let mut next_alt = num_source_glyphs;
    let mut push_alt = |base: u16, pal: u16, class: u16, class_of: &mut HashMap<u16, u16>| -> u16 {
        let alt = next_alt;
        next_alt += 1;
        alt_glyphs.push((base, pal, alt));
        class_of.insert(alt, class);
        alt
    };
    // keyword letters (keyword copies share the base class so keyword
    // rules stay form-insensitive)
    for &c in &keyword_letters {
        if let Some(gid) = char_gid(mappings, c) {
            push_alt(gid, KW, class_of[&gid], &mut class_of);
        }
    }
    if let Some(q) = quote_gid {
        push_alt(q, S_OPEN, c_str_state, &mut class_of);
        push_alt(q, S_CLOSE, c_end_state, &mut class_of);
    }
    if let Some(s) = slash_gid {
        push_alt(s, C_OPEN, c_cmt_state, &mut class_of);
    }
    for &gid in &gids {
        if Some(gid) != quote_gid {
            push_alt(gid, S_BODY, c_str_state, &mut class_of);
        }
    }
    for &gid in &gids {
        push_alt(gid, C_BODY, c_cmt_state, &mut class_of);
    }

    // ---- rules ------------------------------------------------------------
    // Order inside a first-glyph rule set decides who wins at a position:
    // string propagation, comment propagation, then state delimiters and
    // keywords. `//` inside a string therefore stays a string, and `fn`
    // inside a comment stays a comment.
    let mut by_class: BTreeMap<u16, Vec<Rule>> = BTreeMap::new();
    let prop =
        |by_class: &mut BTreeMap<u16, Vec<Rule>>, class: u16, state: u16, pal: u16| {
            by_class.entry(class).or_default().push(Rule {
                backtrack: vec![state],
                input: vec![class],
                lookahead: vec![],
                pal,
            });
        };
    let present: HashSet<u16> = class_of.values().copied().collect();
    for &class in &present {
        if class == c_quote {
            let rules = by_class.entry(class).or_default();
            if quote_gid.is_some() {
                rules.push(Rule {
                    // close: backtrack string state → end-state copy
                    backtrack: vec![c_str_state],
                    input: vec![c_quote],
                    lookahead: vec![],
                    pal: S_CLOSE,
                });
            }
            rules.push(Rule {
                backtrack: vec![c_cmt_state],
                input: vec![c_quote],
                lookahead: vec![],
                pal: C_BODY,
            });
            if quote_gid.is_some() {
                rules.push(Rule {
                    backtrack: vec![],
                    input: vec![c_quote],
                    lookahead: vec![],
                    pal: S_OPEN,
                });
            }
        } else if class == c_slash {
            if quote_gid.is_some() {
                prop(&mut by_class, class, c_str_state, S_BODY);
            }
            prop(&mut by_class, class, c_cmt_state, C_BODY);
            if comment_chars.len() >= 2 {
                // "//" opens the comment state; both slashes substituted
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![],
                    input: vec![c_slash, c_slash],
                    lookahead: vec![],
                    pal: C_OPEN,
                });
            }
        } else {
            if quote_gid.is_some() {
                prop(&mut by_class, class, c_str_state, S_BODY);
            }
            if class != c_nl {
                prop(&mut by_class, class, c_cmt_state, C_BODY);
            }
        }
    }
    // keywords, attached to the rule set of their first letter's class
    for kw in spec.keywords {
        let Some(first) = kw.chars().next() else { continue };
        let Some(&class) = letter_class.get(&first) else {
            continue;
        };
        let input: Vec<u16> = kw
            .chars()
            .map(|c| letter_class[&c])
            .collect();
        by_class.entry(class).or_default().push(Rule {
            backtrack: vec![c_notword],
            input,
            lookahead: vec![c_notword],
            pal: KW,
        });
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
    StrOpen,
    StrBody,
    StrClose,
    CmtOpen,
    CmtBody,
}

fn slot_of(tok: Tok) -> Option<u16> {
    Some(match tok {
        Tok::Plain => return None,
        Tok::Keyword => KW,
        Tok::StrOpen => S_OPEN,
        Tok::StrBody => S_BODY,
        Tok::StrClose => S_CLOSE,
        Tok::CmtOpen => C_OPEN,
        Tok::CmtBody => C_BODY,
    })
}

/// Highlight `text` per the spec, one [`Tok`] per char. String literals
/// end at the next quote (no escapes), comments at the next newline.
///
/// `mapped` says whether the font has a glyph for a char; it models the
/// shaper's real capabilities: keyword boundaries need an *existing,
/// mapped, non-identifier* glyph on both sides — a keyword at the very
/// start/end of the text, or next to an unmapped char (which browsers
/// send to a fallback font, breaking the shaping run), stays plain.
pub fn lex(spec: &LanguageSpec, text: &str, mapped: &dyn Fn(char) -> bool) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = vec![Tok::Plain; chars.len()];
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
                    // the comment ends at a newline, and dies at any char
                    // the font cannot shape (run break in browsers: the
                    // propagation chain cannot cross such a char)
                    if c == '\n' || c == '\r' || !mapped(c) {
                        break;
                    }
                    out[i] = Tok::CmtBody;
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
                    if !mapped(chars[i]) {
                        break; // barrier: state dies, plain scan resumes
                    }
                    out[i] = Tok::StrBody;
                    i += 1;
                }
                continue;
            }
        }
        if is_ident(chars[i]) {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if spec.keywords.contains(&word.as_str()) {
                // A keyword colors only when both neighbors are *plain
                // punctuation* glyphs (the `not-a-word-char` class): the
                // shaper must find a real glyph on each side, and quotes,
                // slashes and newlines live in their own classes. In real
                // code keywords sit between spaces, parens and braces,
                // which is exactly this class.
                let boundary = |c: char| {
                    mapped(c)
                        && !is_ident(c)
                        && Some(c) != spec.string_quote
                        && Some(c) != spec.line_comment.and_then(|s| s.chars().next())
                        && c != '\n'
                        && c != '\r'
                };
                let before = start > 0 && boundary(chars[start - 1]);
                let after = i < chars.len() && boundary(chars[i]);
                if before && after {
                    for tok in out.iter_mut().take(i).skip(start) {
                        *tok = Tok::Keyword;
                    }
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
    ];
    let pieces = [
        "fn", "let", "\"ab\"", "\"fn\"", "// c\n", "/", "x", "_y1", " ", "=", ";", "\"", "//",
        "\"str // inner\"", "mut", "pub", "\n", "s", "use",
    ];
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..300 {
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
        // adjacent to the quote/slash classes it stays plain (their glyphs
        // live outside the not-a-word-char class)
        let t = toks("a let\" b");
        assert_eq!(&t[2..5], &[Tok::Plain; 3]);
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
            &[Tok::Plain; 2],
            "newline lives in its own class, so a keyword right after it stays plain"
        );
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
