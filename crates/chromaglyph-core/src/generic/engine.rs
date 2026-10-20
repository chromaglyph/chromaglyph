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
    /// slot per string kind with interpolation (0 when none): the opener
    /// keeps the string color, the expression body renders uncolored
    interp_open: Vec<u16>,
    interp_body: Vec<u16>,
    num: u16,
    /// slot per span kind (only spans with a close use `span_close`)
    span_open: Vec<u16>,
    span_body: Vec<u16>,
    span_close: Vec<u16>,
    /// slots per markup tag kind: the three colored runs plus the plain
    /// state carriers (delimiters ride uncolored, still carrying the
    /// state — the interpolation-body trick)
    tag_name: Vec<u16>,
    tag_name_plain: Vec<u16>,
    tag_attr: Vec<u16>,
    tag_attr_plain: Vec<u16>,
    tag_val: Vec<u16>,
    tag_val_plain: Vec<u16>,
    /// one quoted-value slot per (tag kind, value quote) — the quote
    /// kinds must not share a state or `"` inside `'…'` would terminate
    tag_valstr: Vec<Vec<u16>>,
    tag_close: Vec<u16>,
    /// slots per block kind: selector words (colored), the value
    /// regime's plain carriers, the value-regime exits
    block_sel: Vec<u16>,
    block_val: Vec<u16>,
    block_end: Vec<u16>,
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
    let mut interp_open = vec![0u16; spec.strings.len()];
    let mut interp_body = vec![0u16; spec.strings.len()];
    for (k, s) in spec.strings.iter().enumerate() {
        str_open.push(next);
        str_body.push(next + 1);
        str_close.push(next + 2);
        next += 3;
        if s.interpolation.is_some() {
            interp_open[k] = next;
            interp_body[k] = next + 1;
            next += 2;
        }
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
    let mut tag_name = Vec::with_capacity(spec.tags.len());
    let mut tag_name_plain = Vec::with_capacity(spec.tags.len());
    let mut tag_attr = Vec::with_capacity(spec.tags.len());
    let mut tag_attr_plain = Vec::with_capacity(spec.tags.len());
    let mut tag_val = Vec::with_capacity(spec.tags.len());
    let mut tag_val_plain = Vec::with_capacity(spec.tags.len());
    let mut tag_valstr = Vec::with_capacity(spec.tags.len());
    let mut tag_close = Vec::with_capacity(spec.tags.len());
    for tag in spec.tags {
        tag_name.push(next);
        tag_name_plain.push(next + 1);
        tag_attr.push(next + 2);
        tag_attr_plain.push(next + 3);
        tag_val.push(next + 4);
        tag_val_plain.push(next + 5);
        let mut quotes = Vec::with_capacity(tag.value_quotes.len());
        for q in 0..tag.value_quotes.len() {
            quotes.push(next + 6 + q as u16);
        }
        tag_valstr.push(quotes);
        tag_close.push(next + 6 + tag.value_quotes.len() as u16);
        next += 7 + tag.value_quotes.len() as u16;
    }
    let mut block_sel = Vec::with_capacity(spec.blocks.len());
    let mut block_val = Vec::with_capacity(spec.blocks.len());
    let mut block_end = Vec::with_capacity(spec.blocks.len());
    for _ in spec.blocks {
        block_sel.push(next);
        block_val.push(next + 1);
        block_end.push(next + 2);
        next += 3;
    }
    Layout {
        kw,
        str_open,
        str_body,
        str_close,
        interp_open,
        interp_body,
        num,
        span_open,
        span_body,
        span_close,
        tag_name,
        tag_name_plain,
        tag_attr,
        tag_attr_plain,
        tag_val,
        tag_val_plain,
        tag_valstr,
        tag_close,
        block_sel,
        block_val,
        block_end,
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
            Tok::InterpOpen(k) => self.interp_open[k],
            Tok::InterpBody(k) => self.interp_body[k],
            Tok::SpanOpen(j) => self.span_open[j],
            Tok::SpanBody(j) => self.span_body[j],
            Tok::SpanClose(j) => self.span_close[j],
            Tok::TagOpen(t) => self.tag_name_plain[t],
            Tok::TagName(t) => self.tag_name[t],
            Tok::TagAttr(t) => self.tag_attr[t],
            Tok::TagAttrPlain(t) => self.tag_attr_plain[t],
            Tok::TagVal(t) => self.tag_val[t],
            Tok::TagValPlain(t) => self.tag_val_plain[t],
            Tok::TagValStr(t, q) => self.tag_valstr[t][q],
            Tok::TagClose(t) => self.tag_close[t],
            Tok::Sel(b) => self.block_sel[b],
            Tok::Val(b) => self.block_val[b],
            Tok::ValEnd(b) => self.block_end[b],
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
        slots[layout.str_close[k] as usize] = colors.clone();
        if s.interpolation.is_some() {
            // the `${` opener keeps the string color; the expression body
            // paints the default foreground — visually plain, but its
            // alt glyphs still carry the interpolation state class
            slots[layout.interp_open[k] as usize] = colors;
            slots[layout.interp_body[k] as usize] = vec![None; themes.len()];
        }
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
    for (t, tag) in spec.tags.iter().enumerate() {
        let name: Vec<Option<Color>> = themes
            .iter()
            .map(|th| th.scope_color(tag.name_scope))
            .collect();
        let attr: Vec<Option<Color>> = themes
            .iter()
            .map(|th| th.scope_color(tag.attr_scope))
            .collect();
        let value: Vec<Option<Color>> = themes
            .iter()
            .map(|th| th.scope_color(tag.value_scope))
            .collect();
        // plain carriers paint the default foreground — visually plain,
        // but their alt glyphs still carry the state class
        let none: Vec<Option<Color>> = vec![None; themes.len()];
        slots[layout.tag_name[t] as usize] = name;
        slots[layout.tag_name_plain[t] as usize] = none.clone();
        slots[layout.tag_attr[t] as usize] = attr;
        slots[layout.tag_attr_plain[t] as usize] = none.clone();
        slots[layout.tag_val[t] as usize] = value.clone();
        slots[layout.tag_val_plain[t] as usize] = none.clone();
        for &q_slot in &layout.tag_valstr[t] {
            slots[q_slot as usize] = value.clone();
        }
        slots[layout.tag_close[t] as usize] = none;
    }
    for (b, block) in spec.blocks.iter().enumerate() {
        slots[layout.block_sel[b] as usize] = themes
            .iter()
            .map(|th| th.scope_color(block.sel_scope))
            .collect();
        let none: Vec<Option<Color>> = vec![None; themes.len()];
        slots[layout.block_val[b] as usize] = none.clone();
        slots[layout.block_end[b] as usize] = none;
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
    for block in spec.blocks {
        for ch in std::iter::once(block.colon).chain(block.ends.iter().copied()) {
            char_class.entry(ch).or_insert_with(|| {
                let id = next_class;
                next_class += 1;
                id
            });
        }
    }
    // markup tag delimiters (`<`, `>`, `/`, `=`, the value quotes) need
    // 1:1 classes: rules key on exact delimiter sequences, and a shared
    // not-a-word class would make every space a potential terminator
    for tag in spec.tags {
        for ch in tag
            .open
            .chars()
            .chain(tag.close.chars())
            .chain(tag.self_close.chars())
            .chain([tag.equals])
            .chain(tag.value_quotes.iter().copied())
        {
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
    // interpolation braces get their own classes in specs that use holes:
    // the opener rule must see the exact `${` sequence (a shared
    // not-a-word class would match `$` + any punctuation) and the resume
    // rule must fire on `}` alone. Cost: `){return` compact style loses
    // its anchor — braces stop being keyword boundaries in those languages
    let any_interp = spec.strings.iter().any(|s| s.interpolation.is_some());
    let mut interp_claimed_ident: HashSet<char> = HashSet::new();
    let c_close_brace = if any_interp {
        let mut claim = |ch: char, next_class: &mut u16| -> u16 {
            *char_class.entry(ch).or_insert_with(|| {
                let id = *next_class;
                *next_class += 1;
                id
            })
        };
        for s in spec.strings.iter() {
            if let Some((open, close)) = s.interpolation {
                for ch in open.chars().chain([close]) {
                    claim(ch, &mut next_class);
                }
            }
        }
        for ch in char_class.keys() {
            if is_ident(*ch, spec.ident_extra) {
                interp_claimed_ident.insert(*ch);
            }
        }
        Some(*char_class.get(&'}').expect("claimed above"))
    } else {
        None
    };
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
    // digits that appear inside keywords (C++ `char8_t`) carry keyword
    // classes — they must still OPEN a number after a boundary like any
    // other digit, or `8;` would stay plain while `48;` colors
    let digit_letter_classes: Vec<u16> = keyword_letters
        .iter()
        .filter(|c| c.is_ascii_digit())
        .map(|&c| letter_class[&c])
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
    // one interpolation state per kind that has holes; kinds without
    // holes never reference theirs
    let interp_state: Vec<u16> = (0..spec.strings.len())
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
    // markup tag states: name / attr regime / unquoted value / quoted
    // value, four per tag kind
    let mut tag_name_state = Vec::with_capacity(spec.tags.len());
    let mut tag_attr_state = Vec::with_capacity(spec.tags.len());
    let mut tag_val_state = Vec::with_capacity(spec.tags.len());
    // one quoted-value state per (tag kind, value quote): `"` inside
    // `'…'` is body, only the matching quote terminates
    let mut tag_vstr_state = Vec::with_capacity(spec.tags.len());
    for tag in spec.tags {
        for v in [&mut tag_name_state, &mut tag_attr_state, &mut tag_val_state] {
            let id = next_class;
            next_class += 1;
            v.push(id);
        }
        let mut vstr = Vec::with_capacity(tag.value_quotes.len());
        for _ in tag.value_quotes {
            let id = next_class;
            next_class += 1;
            vstr.push(id);
        }
        tag_vstr_state.push(vstr);
    }
    let mut block_sel_state = Vec::with_capacity(spec.blocks.len());
    let mut block_val_state = Vec::with_capacity(spec.blocks.len());
    for _ in spec.blocks {
        for v in [&mut block_sel_state, &mut block_val_state] {
            let id = next_class;
            next_class += 1;
            v.push(id);
        }
    }
    // letter carriers of the value regime: word-ish glyphs riding in a
    // value keep their own state class (distinct from the space/colon
    // carriers), so a keyword/number rule with an empty backtrack can
    // tell a mid-value word (`zmedia`) from a word start (`x: media`) —
    // the guard substitutes on the letter-carrier class only
    let c_val_ident: Vec<u16> = (0..spec.blocks.len())
        .map(|_| {
            let id = next_class;
            next_class += 1;
            id
        })
        .collect();
    // the merged context class: backtrack/lookahead ClassDefs collapse
    // every word-ish letter class into this one id, so a guard rule says
    // "after any word char" with a single entry instead of one rule per
    // (keyword × word class). Digits stay their own class — the number
    // rules key on "digit follows" exactly
    let c_wordish = next_class;
    next_class += 1;
    let c_num_state = next_class;
    next_class += 1;
    let c_end_state = next_class;

    const C_NOTWORD: u16 = 1;
    const C_IDENT: u16 = 2;

    // word-ish classes by CHARACTER, not by class identity: a char that
    // continues names/attrs/values may still carry a 1:1 delimiter class
    // (html's `-` is both a data-attribute char and a `<!--` span char)
    let wordish_classes: HashSet<u16> = {
        let mut set = HashSet::new();
        set.insert(C_IDENT);
        set.insert(c_digit);
        for (&ch, &cls) in &char_class {
            if is_ident(ch, spec.ident_extra) || ch.is_ascii_digit() {
                set.insert(cls);
            }
        }
        // keyword letters carry 1:1 classes — in keyword languages they
        // are the word glyphs (a CSS selector word is all letter classes)
        for &cls in letter_class.values() {
            set.insert(cls);
        }
        set
    };
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
        } else if let Some(&id) = letter_class.get(&ch) {
            // keyword letters BEFORE the digit test: a digit inside a
            // keyword (`char8_t`) must carry its keyword class, or the
            // keyword rule's input sequence can never match what the
            // shaper sees. For letter-only keywords this is the same
            // class the is_ident branch below would pick
            id
        } else if ch.is_ascii_digit() {
            c_digit
        } else if is_ident(ch, spec.ident_extra) {
            C_IDENT
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
                let class = if group.value && !spec.blocks.is_empty() {
                    // value-position keywords keep the value regime
                    // alive through their own glyphs (as letter carriers)
                    c_val_ident[0]
                } else {
                    class_of[&gid]
                };
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
    // interpolation copies: the opener glyphs (string-colored, entering
    // the interpolation state) and one uncolored copy of every glyph the
    // expression may contain — the closer reuses the string body slot
    for (k, s) in spec.strings.iter().enumerate() {
        let Some((open, close)) = s.interpolation else {
            continue;
        };
        if quote_gid[k].is_none() {
            continue;
        }
        let mut seen_open: HashSet<u16> = HashSet::new();
        for ch in open.chars() {
            if let Some(gid) = char_gid(mappings, ch) {
                if seen_open.insert(gid) {
                    push_alt(
                        &mut next_new,
                        &mut alt_glyphs,
                        gid,
                        layout.interp_open[k],
                        interp_state[k],
                        &mut class_of,
                    );
                }
            }
        }
        let _ = close;
        for &gid in &gids {
            let class = class_of[&gid];
            if class != c_notdef && class != c_nl && Some(class) != c_close_brace {
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.interp_body[k],
                    interp_state[k],
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
    // markup tag copies: opener carriers (`<`, the `/` of `</` and of
    // `/>` reuse the name-plain slot), colored word runs, plain state
    // carriers (attr regime / quoted value need one of nearly every
    // glyph), and the close delimiters
    for (t, tag) in spec.tags.iter().enumerate() {
        let wordish = |class: u16| wordish_classes.contains(&class);
        let mut open_carriers: HashSet<u16> = HashSet::new();
        for ch in tag.open.chars().chain(tag.self_close.chars()) {
            if let Some(gid) = char_gid(mappings, ch) {
                if open_carriers.insert(gid) {
                    push_alt(
                        &mut next_new,
                        &mut alt_glyphs,
                        gid,
                        layout.tag_name_plain[t],
                        tag_name_state[t],
                        &mut class_of,
                    );
                }
            }
        }
        // kind-major (contiguous same-class alts — see the block loop)
        for &gid in &gids {
            let class = class_of[&gid];
            if class != c_notdef && class != c_nl && wordish(class) {
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.tag_name[t],
                    tag_name_state[t],
                    &mut class_of,
                );
            }
        }
        for &gid in &gids {
            let class = class_of[&gid];
            if class != c_notdef && class != c_nl && wordish(class) {
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.tag_attr[t],
                    tag_attr_state[t],
                    &mut class_of,
                );
            }
        }
        for &gid in &gids {
            let class = class_of[&gid];
            if class != c_notdef && class != c_nl && wordish(class) {
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.tag_val[t],
                    tag_val_state[t],
                    &mut class_of,
                );
            }
        }
        // the attr regime's plain carriers (spaces, `/`, `=`
        // transitions, closing quotes) and the quoted value bodies
        // (quotes of the other kind included in each — only the
        // matching quote terminates, by rule order)
        for &gid in &gids {
            let class = class_of[&gid];
            if class != c_notdef && class != c_nl {
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.tag_attr_plain[t],
                    tag_attr_state[t],
                    &mut class_of,
                );
            }
        }
        for (q, _) in tag.value_quotes.iter().enumerate() {
            for &gid in &gids {
                let class = class_of[&gid];
                if class != c_notdef && class != c_nl {
                    push_alt(
                        &mut next_new,
                        &mut alt_glyphs,
                        gid,
                        layout.tag_valstr[t][q],
                        tag_vstr_state[t][q],
                        &mut class_of,
                    );
                }
            }
        }
        if let Some(gid) = char_gid(mappings, tag.equals) {
            push_alt(
                &mut next_new,
                &mut alt_glyphs,
                gid,
                layout.tag_val_plain[t],
                tag_val_state[t],
                &mut class_of,
            );
        }
        let mut closers: HashSet<u16> = HashSet::new();
        for ch in tag.close.chars().chain(tag.self_close.chars()) {
            if let Some(gid) = char_gid(mappings, ch) {
                if closers.insert(gid) {
                    push_alt(
                        &mut next_new,
                        &mut alt_glyphs,
                        gid,
                        layout.tag_close[t],
                        c_end_state,
                        &mut class_of,
                    );
                }
            }
        }
    }
    // block (property-sheet) copies: selector-colored word glyphs and
    // the value regime's plain carriers (one of nearly every glyph),
    // plus the exit delimiters
    for (b, block) in spec.blocks.iter().enumerate() {
        // kind-major on purpose: same-class alts get contiguous glyph
        // ids, so the chain subtables' ClassDefs collapse into a
        // handful of ranges instead of one per glyph
        for &gid in &gids {
            let class = class_of[&gid];
            if class != c_notdef && class != c_nl && wordish_classes.contains(&class) {
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.block_sel[b],
                    block_sel_state[b],
                    &mut class_of,
                );
            }
        }
        for &gid in &gids {
            let class = class_of[&gid];
            if class != c_notdef && class != c_nl {
                // word-ish carriers are letter carriers (their own class);
                // everything else rides as an ordinary value carrier
                let carrier = if wordish_classes.contains(&class) {
                    c_val_ident[b]
                } else {
                    block_val_state[b]
                };
                push_alt(
                    &mut next_new,
                    &mut alt_glyphs,
                    gid,
                    layout.block_val[b],
                    carrier,
                    &mut class_of,
                );
            }
        }
        let mut ends: HashSet<u16> = HashSet::new();
        if let Some(gid) = char_gid(mappings, block.colon) {
            ends.insert(gid);
        }
        for &ch in block.ends {
            if let Some(gid) = char_gid(mappings, ch) {
                ends.insert(gid);
            }
        }
        for gid in ends {
            push_alt(
                &mut next_new,
                &mut alt_glyphs,
                gid,
                layout.block_end[b],
                c_end_state,
                &mut class_of,
            );
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
            pal: Some(pal),
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
            pal: Some(layout.span_close[j]),
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
            pal: Some(layout.str_close[k]),
        });
    }
    // markup tag exits: `>` / `/>` from the name, attr and value states
    // end the tag; the closing value quote returns to the attr regime;
    // a fresh `<x` inside an unfinished tag abandons it (re-opens)
    for (t, tag) in spec.tags.iter().enumerate() {
        let tag_states = [tag_name_state[t], tag_attr_state[t], tag_val_state[t]];
        for &state in &tag_states {
            if let (Some(&sl), Some(&gt)) = (
                tag.self_close
                    .chars()
                    .next()
                    .and_then(|c| char_class.get(&c)),
                tag.close.chars().next().and_then(|c| char_class.get(&c)),
            ) {
                if tag.self_close.chars().count() > 1 {
                    let input: Vec<u16> = tag.self_close.chars().map(|c| char_class[&c]).collect();
                    terminators.entry(sl).or_default().push(Rule {
                        backtrack: vec![state],
                        input,
                        lookahead: vec![],
                        pal: Some(layout.tag_close[t]),
                    });
                }
                terminators.entry(gt).or_default().push(Rule {
                    backtrack: vec![state],
                    input: vec![gt],
                    lookahead: vec![],
                    pal: Some(layout.tag_close[t]),
                });
            }
        }
        for (q, &qc) in tag.value_quotes.iter().enumerate() {
            let Some(&class) = char_class.get(&qc) else {
                continue;
            };
            terminators.entry(class).or_default().push(Rule {
                backtrack: vec![tag_vstr_state[t][q]],
                input: vec![class],
                lookahead: vec![],
                pal: Some(layout.tag_attr_plain[t]),
            });
        }
        if let Some(&lt) = tag.open.chars().next().and_then(|c| char_class.get(&c)) {
            for &state in &tag_states[1..] {
                terminators.entry(lt).or_default().push(Rule {
                    backtrack: vec![state],
                    input: vec![lt],
                    lookahead: vec![C_IDENT],
                    pal: Some(layout.tag_name_plain[t]),
                });
            }
        }
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
            pal: Some(layout.span_open[j]),
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
            pal: Some(layout.str_open[k]),
        });
    }
    // markup tag openings: `<` + identifier lookahead (a lone `<` is
    // text, `<!--` is a span opener that sorts longer-first), and the
    // `</` closing-tag prefix rides the same plain carrier slot
    for (t, tag) in spec.tags.iter().enumerate() {
        let Some(&lt) = tag.open.chars().next().and_then(|c| char_class.get(&c)) else {
            continue;
        };
        openers.entry(lt).or_default().push(Rule {
            backtrack: vec![],
            input: vec![lt],
            lookahead: vec![C_IDENT],
            pal: Some(layout.tag_name_plain[t]),
        });
        if let Some(&sl) = tag
            .self_close
            .chars()
            .next()
            .and_then(|c| char_class.get(&c))
        {
            if sl != lt {
                // `</name` — the slash is the self-close's first char
                openers.entry(lt).or_default().push(Rule {
                    backtrack: vec![],
                    input: vec![lt, sl],
                    lookahead: vec![C_IDENT],
                    pal: Some(layout.tag_name_plain[t]),
                });
            }
        }
    }
    for rules in openers.values_mut() {
        rules.sort_by_key(|r| std::cmp::Reverse(r.input.len()));
    }

    // interpolation: the opener substitutes its glyphs (string-colored)
    // into the interpolation state; the closer resumes the string via the
    // ordinary body slot. Nested openers stay expression body — the
    // first closer wins, flat by design
    let mut interp_openers: HashMap<u16, Vec<Rule>> = HashMap::new();
    let mut interp_resume: HashMap<u16, Vec<Rule>> = HashMap::new();
    for (k, s) in spec.strings.iter().enumerate() {
        let Some((open, close)) = s.interpolation else {
            continue;
        };
        if quote_gid[k].is_none() {
            continue;
        }
        let mut input: Vec<u16> = Vec::new();
        let mut ok = !open.is_empty();
        for ch in open.chars() {
            match char_class.get(&ch) {
                Some(&cls) => input.push(cls),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        let Some(&close_cls) = char_class.get(&close) else {
            continue;
        };
        if !ok {
            continue;
        }
        interp_openers.entry(input[0]).or_default().push(Rule {
            backtrack: vec![str_state[k]],
            input,
            lookahead: vec![],
            pal: Some(layout.interp_open[k]),
        });
        interp_resume.entry(close_cls).or_default().push(Rule {
            backtrack: vec![interp_state[k]],
            input: vec![close_cls],
            lookahead: vec![],
            pal: Some(layout.str_body[k]),
        });
    }
    for rules in interp_openers.values_mut() {
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
    let claimed_ident_classes: HashSet<u16> = interp_claimed_ident
        .iter()
        .filter_map(|ch| char_class.get(ch).copied())
        .collect();
    let num_body = |class: u16| -> bool {
        spec.numbers
            && digit_gid.is_some()
            && (class == c_digit
                || class == C_IDENT
                || claimed_ident_classes.contains(&class)
                || (class >= first_letter_class
                    && class < first_letter_class + keyword_letters.len() as u16))
    };
    // keyword boundary classes: plain punctuation only, plus — for
    // property-sheet (block) languages — the value-regime exits/carriers
    // and the value state itself (`!important` after a carried `!`). A
    // newline can never anchor a keyword: the previous line is a
    // different shaping run in every browser, so a column-0 keyword has
    // no in-run glyph before it
    // boundaries stay SMALL on purpose: every entry multiplies the
    // keyword rule count (a CSS-sized property list × N anchors). The
    // plain space/punctuation class, the generic end state (a value
    // exit `;`/`}` — also any string/comment close), a plain `{` (the
    // first property of a block), and the value state (`!important`
    // after a carried `!`). Plain `;`/`}` anchor nothing: what follows
    // them in real CSS follows a *terminated* value (end state), not a
    // bare delimiter.
    let mut boundaries = vec![C_NOTWORD];
    if !spec.blocks.is_empty() {
        boundaries.push(c_end_state);
    }
    for (b, block) in spec.blocks.iter().enumerate() {
        boundaries.push(block_val_state[b]);
        for ch in std::iter::once(block.open).chain(block.ends.iter().copied()) {
            if let Some(&cls) = char_class.get(&ch) {
                boundaries.push(cls);
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    // selector opens anchor on plain punctuation, the end state, and the
    // anchors (`.`, `:`): `.foo` and `:hover` open after their
    // punctuation; the value state must not start a selector
    let mut sel_anchor: Vec<u16> = boundaries
        .iter()
        .copied()
        .filter(|&c| !block_val_state.contains(&c))
        .collect();
    for block in spec.blocks {
        for ch in block.anchors {
            if let Some(&cls) = char_class.get(ch) {
                sel_anchor.push(cls);
            }
        }
    }
    sel_anchor.sort_unstable();
    sel_anchor.dedup();

    // keywords longest-first, shared by every class's rule set: a longer
    // keyword sharing a first letter with a shorter one (`async` / `as`)
    // must get its rule before the shorter one's continuation guard,
    // which blocks the rest of the rule set at that position
    let mut kw_longest_first: Vec<(usize, &str)> = spec
        .keywords
        .iter()
        .enumerate()
        .flat_map(|(g, group)| group.words.iter().map(move |&w| (g, w)))
        .collect();
    kw_longest_first.sort_by_key(|&(_, w)| std::cmp::Reverse(w.chars().count()));

    // every class a word-ish glyph may carry in a backtrack/lookahead
    // window: the merged word class, plain digits, and the (unmerged)
    // keyword-letter classes of embedded digits
    let word_ctx_classes = || {
        let mut v = vec![c_wordish, c_digit];
        v.extend(digit_letter_classes.iter().copied());
        v
    };

    for &class in &present {
        if let Some(rules) = terminators.get(&class) {
            by_class
                .entry(class)
                .or_default()
                .extend(rules.iter().cloned());
        }
        if let Some(rules) = interp_openers.get(&class) {
            by_class
                .entry(class)
                .or_default()
                .extend(rules.iter().cloned());
        }
        if let Some(rules) = interp_resume.get(&class) {
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
                        pal: Some(layout.str_body[k]),
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
        // expression contents propagate the interpolation state uncolored;
        // the closer's class is excluded — the resume rule owns it
        for (k, s) in spec.strings.iter().enumerate() {
            if s.interpolation.is_none() || quote_gid[k].is_none() {
                continue;
            }
            if class != c_notdef && class != c_nl && Some(class) != c_close_brace {
                prop(
                    &mut by_class,
                    class,
                    interp_state[k],
                    layout.interp_body[k],
                    vec![],
                );
            }
        }
        // markup tag state machine, per class: word classes color in the
        // name/attr/value states; everything else hands the state on via
        // plain carriers (`>`/`/>`/quote exits live in the terminator
        // section above; the equals rule and the unquoted→quoted switch
        // are the only class-specific transitions)
        for (t, tag) in spec.tags.iter().enumerate() {
            let wordish = wordish_classes.contains(&class);
            if wordish {
                prop(
                    &mut by_class,
                    class,
                    tag_name_state[t],
                    layout.tag_name[t],
                    vec![],
                );
                prop(
                    &mut by_class,
                    class,
                    tag_attr_state[t],
                    layout.tag_attr[t],
                    vec![],
                );
                prop(
                    &mut by_class,
                    class,
                    tag_val_state[t],
                    layout.tag_val[t],
                    vec![],
                );
            } else if char_class.get(&tag.equals) == Some(&class) {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![tag_attr_state[t]],
                    input: vec![class],
                    lookahead: vec![],
                    pal: Some(layout.tag_val_plain[t]),
                });
            }
            if !wordish {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![tag_name_state[t]],
                    input: vec![class],
                    lookahead: vec![],
                    pal: Some(layout.tag_attr_plain[t]),
                });
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![tag_attr_state[t]],
                    input: vec![class],
                    lookahead: vec![],
                    pal: Some(layout.tag_attr_plain[t]),
                });
                if let Some(q) = tag
                    .value_quotes
                    .iter()
                    .position(|qc| char_class.get(qc) == Some(&class))
                {
                    // an unquoted value switches into its quoted form
                    by_class.entry(class).or_default().push(Rule {
                        backtrack: vec![tag_val_state[t]],
                        input: vec![class],
                        lookahead: vec![],
                        pal: Some(layout.tag_valstr[t][q]),
                    });
                } else {
                    by_class.entry(class).or_default().push(Rule {
                        backtrack: vec![tag_val_state[t]],
                        input: vec![class],
                        lookahead: vec![],
                        pal: Some(layout.tag_attr_plain[t]),
                    });
                }
            }
            // quoted-value bodies: everything continues colored in the
            // owning quote's state (the matching quote terminates by
            // rule order)
            for (q, _) in tag.value_quotes.iter().enumerate() {
                prop(
                    &mut by_class,
                    class,
                    tag_vstr_state[t][q],
                    layout.tag_valstr[t][q],
                    vec![],
                );
            }
        }
        if spec.numbers && class == c_dot {
            // `.` continues a number only when a digit follows: 3.14 yes,
            // 1.max stops before the dot. The lookahead enumerates every
            // class a digit GLYPH carries — plain digits share c_digit,
            // but digits inside keywords (`char16_t`'s 1) carry their
            // keyword letter classes (unmerged, exactly for this)
            for la in std::iter::once(c_digit).chain(digit_letter_classes.iter().copied()) {
                prop(&mut by_class, class, c_num_state, layout.num, vec![la]);
            }
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
        // ---- negative-context guards + empty-context rules --------------
        // The keyword and number rules below carry EMPTY backtrack and
        // lookahead sequences. An empty context is not "no context": it
        // matches unconditionally, including at a run start — and a
        // browser shapes every source line of a `pre` as its own run, so
        // a column-0 keyword has no glyph behind it that a boundary
        // backtrack could ever see. The price is that an empty backtrack
        // also matches mid-word, which no boundary enumeration can
        // exclude; the guards below (earlier in the same rule set — the
        // shaper stops at the first match) supply those negatives, the
        // compiled form of AFDKO's `ignore sub` idiom.
        let digit_rule_class = class == c_digit || digit_letter_classes.contains(&class);

        // selector-word continuation, before any keyword rule: a
        // mid-word selector letter is consumed here, so no keyword can
        // fire inside a selector word
        for (b, _block) in spec.blocks.iter().enumerate() {
            if wordish_classes.contains(&class) {
                prop(
                    &mut by_class,
                    class,
                    block_sel_state[b],
                    layout.block_sel[b],
                    vec![],
                );
            }
        }
        // value regime, mid-word: after a letter carrier the glyph rides
        // on as a carrier (never a keyword or number)
        for (b, _block) in spec.blocks.iter().enumerate() {
            if wordish_classes.contains(&class) {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![c_val_ident[b]],
                    input: vec![class],
                    lookahead: vec![],
                    pal: Some(layout.block_val[b]),
                });
            }
        }
        // non-block languages: after any word char (the merged class) or
        // a bare digit, the glyph starts no keyword and no number. The
        // zero-record rule matches and blocks without substituting
        let kw_starts = spec.keywords.iter().any(|g| {
            g.words
                .iter()
                .any(|w| w.chars().next().and_then(|c| letter_class.get(&c)) == Some(&class))
        });
        if spec.blocks.is_empty() && (kw_starts || digit_rule_class) {
            for bt in word_ctx_classes() {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: vec![bt],
                    input: vec![class],
                    lookahead: vec![],
                    pal: None,
                });
            }
        }
        // a word that CONTINUES past a keyword never colors it: guard on
        // a word-ish lookahead, per non-followed keyword (a followed_by
        // keyword's exact-char lookahead is its own guard). Keywords
        // order LONGEST FIRST with the coloring rule directly after its
        // guard: `as` must not poison `async` at the same position —
        // the guard matches `as` + word-ish and blocks everything later
        // in this rule set, so `async`'s rule has to come first
        for &(g, kw) in &kw_longest_first {
            let group = &spec.keywords[g];
            let Some(first) = kw.chars().next() else {
                continue;
            };
            if letter_class.get(&first) != Some(&class) {
                continue;
            }
            let input: Vec<u16> = kw.chars().map(|c| letter_class[&c]).collect();
            // the continuation guard is for plain groups only — a
            // followed_by keyword's exact-char lookahead is its own guard
            if group.followed_by.is_none() {
                for la in word_ctx_classes() {
                    if spec.blocks.is_empty() {
                        by_class.entry(class).or_default().push(Rule {
                            backtrack: vec![],
                            input: input.clone(),
                            lookahead: vec![la],
                            pal: None,
                        });
                    } else {
                        // block languages: the substituted palette must
                        // keep the regime alive — carriers inside a
                        // value, selector colors outside it
                        for bt in [block_val_state[0], c_val_ident[0]] {
                            by_class.entry(class).or_default().push(Rule {
                                backtrack: vec![bt],
                                input: input.clone(),
                                lookahead: vec![la],
                                pal: Some(layout.block_val[0]),
                            });
                        }
                        by_class.entry(class).or_default().push(Rule {
                            backtrack: vec![],
                            input: input.clone(),
                            lookahead: vec![la],
                            pal: Some(layout.block_sel[0]),
                        });
                    }
                }
            }
            // the keyword itself: empty backtrack (a run start is an
            // anchor), empty lookahead for plain groups (a run end is a
            // boundary), the followed char for followed_by groups
            let la: Vec<u16> = match group.followed_by.and_then(|c| char_class.get(&c)) {
                Some(&cls) => vec![cls],
                None => Vec::new(),
            };
            by_class.entry(class).or_default().push(Rule {
                backtrack: Vec::new(),
                input,
                lookahead: la,
                pal: Some(layout.kw[g]),
            });
        }
        // numbers open at a run start like keywords — including on the
        // keyword-embedded digits' classes (char8_t's 8)
        if spec.numbers && digit_rule_class {
            by_class.entry(class).or_default().push(Rule {
                backtrack: Vec::new(),
                input: vec![class],
                lookahead: Vec::new(),
                pal: Some(layout.num),
            });
        }
        // property-sheet (CSS) blocks — after keywords so a property
        // word (keyword + colon lookahead) beats the selector open at
        // the same position
        for (b, block) in spec.blocks.iter().enumerate() {
            let colon_cls = char_class.get(&block.colon).copied();
            let end_cls: Vec<u16> = block
                .ends
                .iter()
                .filter_map(|c| char_class.get(c).copied())
                .collect();
            let wordish = wordish_classes.contains(&class);
            // the open after an anchor — plain boundary, a value exit,
            // or an anchor char
            if wordish {
                for &bt in &sel_anchor {
                    by_class.entry(class).or_default().push(Rule {
                        backtrack: vec![bt],
                        input: vec![class],
                        lookahead: vec![],
                        pal: Some(layout.block_sel[b]),
                    });
                }
            }
            // the colon after ANY word enters the value regime: plain
            // idents, keyword letters (which keep their base letter
            // class) and selector words (which carry the selector
            // state) all land here
            if Some(class) == colon_cls {
                let mut colon_bts = word_ctx_classes();
                colon_bts.push(block_sel_state[b]);
                for bt in colon_bts {
                    by_class.entry(class).or_default().push(Rule {
                        backtrack: vec![bt],
                        input: vec![class],
                        lookahead: vec![],
                        pal: Some(layout.block_val[b]),
                    });
                }
            }
            // value regime: the exit chars terminate (both carrier
            // kinds hand the regime over), everything else rides as
            // plain carriers — and the glyph right after a number hands
            // the regime back (excluded: the exits own their classes)
            if !end_cls.contains(&class) {
                for bt in [block_val_state[b], c_val_ident[b]] {
                    prop(&mut by_class, class, bt, layout.block_val[b], vec![]);
                }
                if spec.numbers {
                    by_class.entry(class).or_default().push(Rule {
                        backtrack: vec![c_num_state],
                        input: vec![class],
                        lookahead: vec![],
                        pal: Some(layout.block_val[b]),
                    });
                }
            } else {
                for bt in [block_val_state[b], c_val_ident[b]] {
                    by_class.entry(class).or_default().push(Rule {
                        backtrack: vec![bt],
                        input: vec![class],
                        lookahead: vec![],
                        pal: Some(layout.block_end[b]),
                    });
                }
                if spec.numbers {
                    // `4px;` — the exit follows the number state
                    by_class.entry(class).or_default().push(Rule {
                        backtrack: vec![c_num_state],
                        input: vec![class],
                        lookahead: vec![],
                        pal: Some(layout.block_end[b]),
                    });
                }
            }
            // the selector open at a run start — last of the class's
            // rules: everything that could own the position (keywords,
            // the value regime's carriers) has had its turn
            if wordish {
                by_class.entry(class).or_default().push(Rule {
                    backtrack: Vec::new(),
                    input: vec![class],
                    lookahead: Vec::new(),
                    pal: Some(layout.block_sel[b]),
                });
            }
        }
    }
    let rules: Vec<Rule> = by_class.into_values().flatten().collect();

    // the backtrack/lookahead ClassDefs merge every word-ish class but
    // the digits into `c_wordish` (see its definition); the input def
    // never merges, so rule sets stay keyed on exact first classes
    let context_merge: HashSet<u16> = wordish_classes
        .iter()
        .copied()
        .filter(|&c| c != c_digit && !digit_letter_classes.contains(&c))
        .collect();

    Ok(GlyphPlan {
        cells: Vec::new(),
        alt_glyphs,
        class_of,
        rules,
        context_merge: Some((c_wordish, context_merge)),
        // the generic state machine has no build-time content to shape:
        // no ligature sites, no per-cell units (nothing consumes them here)
        lig_rules: Vec::new(),
        lig_sites: Vec::new(),
        units: Vec::new(),
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
