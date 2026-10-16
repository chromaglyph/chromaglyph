//! Slot layout + rule synthesis: turn a `LanguageSpec` into a
//! `GlyphPlan` (glyph classes, alt copies, ordered contextual rules).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::compile::{GlyphPlan, Rule};
use crate::error::BuildError;
use crate::spec::Color;
use crate::theme::ThemeSpec;

use super::lex::Tok;
use super::spec::{is_ident, LanguageSpec};

// ---- slot layout ----------------------------------------------------------
// Slots are palette entries AND nested-lookup indices (see gsub). Opening
// and closing delimiters use distinct slots because a nested single-subst
// lookup maps a glyph to exactly one substitute, but both paint the same
// color. State-bearing slots are never shared between kinds: the state
// class lives on the alt glyphs, so merging (say) a line comment with a
// block comment would also merge their newline semantics.
pub(in crate::generic) struct Layout {
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

pub(in crate::generic) fn layout(spec: &LanguageSpec) -> Layout {
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
    pub(in crate::generic) fn tok_slot(&self, tok: Tok) -> Option<u16> {
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
            for (k, s) in spec.strings.iter().enumerate() {
                if quote_gid[k].is_none() || !s.escapes {
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
