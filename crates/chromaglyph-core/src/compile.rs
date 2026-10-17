//! Stage 3 of the pipeline: per-character colors → chain-context GSUB rules.
//!
//! This is the heart of chromaglyph's "exact mode". Because the content is
//! known at build time, every colored token occurrence becomes one contextual
//! substitution rule whose backtrack/lookahead is the *minimal* window that
//! uniquely pins the occurrence in the whole (multi-block) text. There is no
//! lexer state machine to carry through the font, unlike the original
//! FontWithASyntaxHighlighter design.
//!
//! ## Ligature re-materialization (exact mode + a shaper)
//!
//! The source font's own GSUB is dropped from the output font, so its
//! ligatures would be lost. Instead of keeping (and subsetting) the source
//! layout tables, the build *shapes* each block with the source font
//! (rustybuzz — the same algorithm browsers run) and re-materializes every
//! substitution the shaper performed as one more minimal-window rule: the
//! pristine per-char glyphs become the shaped glyph through a nested
//! single/ligature substitution. A site whose member cells color uniformly
//! restores the ligature glyph itself, which is then colored like any base
//! glyph by the ordinary rules; a site with mixed colors degrades to plain
//! per-char text so every character keeps its color.
//!
//! Ordering inside `calt` is mirrored by two coordinate spaces. Ligature
//! rules run first, on the pristine stream: their lookahead is compiled
//! against the pristine glyph sequence (`seq`), while their backtrack sees
//! glyphs already restored by earlier rules (the restored stream `rseq`) —
//! exactly the state a shaper presents mid-lookup. Coloring rules run
//! second, entirely against the restored stream; that is why their
//! contexts may reference ligature glyphs.
//!
//! ## The slot model (multi-theme in one font)
//!
//! Input colors are per character *per theme*. A **slot** is one distinct
//! vector of per-theme colors (deduplicated globally); a char is substituted
//! iff its slot colors anything in any theme (union structure). Each slot
//! becomes one empty colored glyph copy per base glyph whose COLR layer
//! points at palette index = slot. CPAL then carries one palette per theme,
//! so the *same* GSUB rules render correctly under every theme — switching
//! is a pure `@font-palette-values` flip, no re-shaping.
//!
//! Two invariants make the generated rules correct under any shaper that
//! applies `calt` left-to-right (HarfBuzz, rustybuzz, browsers):
//!
//! 1. **Form-insensitive contexts.** Context classes contain the base glyph
//!    *and all of its colored copies*, so a context matches no matter whether
//!    the neighboring token has already been substituted.
//! 2. **Global false-match audit.** After picking a minimal window for an
//!    occurrence, we scan *every* position of every block: if the rule would
//!    also match there and would produce a different glyph sequence than
//!    intended, the window is grown until the false match disappears.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use crate::error::BuildError;
use crate::spec::Color;

/// Glyph-level view of one char cell in the flattened text.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// Byte offset of this char in its block (used as the shaping cluster).
    pub byte_start: usize,
    pub ch: char,
    /// Base glyph id from cmap; `None` for unmapped chars (shape to .notdef).
    pub gid: Option<u16>,
    /// Index of the owning block.
    pub block: usize,
    /// Slot id after substitution; `None` = keep base glyph (uncolored
    /// in every theme).
    pub pal: Option<u16>,
}

/// One glyph of build-time shaping ground truth (rustybuzz over the source
/// font): the glyph id and the byte offset of the cluster it descends from.
#[derive(Clone, Copy, Debug)]
pub struct ShapedGlyph {
    pub glyph: u16,
    pub cluster: u32,
}

/// Build-time shaping oracle: block text → the glyphs the source font's own
/// shaping produces, aligned by cluster.
pub type Shaper<'a> = &'a dyn Fn(&str) -> Vec<ShapedGlyph>;

/// One contextual ligature-restore rule, expressed in class ids. The nested
/// substitution is the per-site lookup (see [`GlyphPlan::lig_sites`]),
/// applied at input position 0.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LigRule {
    /// Backtrack classes (restored-stream glyphs), outermost (leftmost) first.
    pub backtrack: Vec<u16>,
    /// Input classes for the full member span (first glyph included).
    pub input: Vec<u16>,
    /// Lookahead classes (pristine-stream glyphs), innermost first.
    pub lookahead: Vec<u16>,
    /// Index into [`GlyphPlan::lig_sites`].
    pub site: u16,
}

/// A re-materialized substitution site: `components` (the pristine per-char
/// glyphs the text resolves to) become `out` (the glyph the source font's
/// own shaping produced). A single component is a contextual single
/// substitution; several form a ligature.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LigSite {
    pub components: Vec<u16>,
    pub out: u16,
}

/// One glyph the output font produces for a block: a plain cell, or a
/// re-materialized substitution site covering several cells.
#[derive(Clone, Copy, Debug)]
pub struct Unit {
    /// First covered cell (index into `GlyphPlan::cells`); the unit spans
    /// `len` cells from it.
    pub cell: u32,
    pub len: u32,
    /// Glyph after shape-restore, before coloring.
    pub glyph: u16,
    /// Slot if the covered cells color uniformly.
    pub pal: Option<u16>,
    pub block: usize,
    /// Position in the restored stream (the coloring rules' space).
    pub rpos: u32,
}

/// One contextual substitution rule, expressed in class ids.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rule {
    /// Backtrack classes, outermost (leftmost) first.
    pub backtrack: Vec<u16>,
    /// Input classes for the full token span (first glyph included).
    pub input: Vec<u16>,
    /// Lookahead classes, innermost (rightmost of token) first.
    pub lookahead: Vec<u16>,
    /// Slot index substituted onto every input position.
    pub pal: u16,
}

/// Everything the font builder and the verifier need to know about the plan:
/// colored glyph copies, the class definition, and the final rule set.
#[derive(Debug)]
pub struct GlyphPlan {
    pub cells: Vec<Cell>,
    /// (base_gid, slot, alt_gid) sorted by (base_gid, slot);
    /// alt gids continue after the source font's glyph ids.
    pub alt_glyphs: Vec<(u16, u16, u16)>,
    /// glyph id -> class id (base glyphs and their alt copies share a class).
    pub class_of: HashMap<u16, u16>,
    pub rules: Vec<Rule>,
    /// Ligature-restore rules (run before `rules` inside `calt`).
    pub lig_rules: Vec<LigRule>,
    /// Deduplicated substitution sites the ligature rules reference.
    pub lig_sites: Vec<LigSite>,
    /// The shaping units: one entry per glyph the output font produces per
    /// block, in stream order (the coloring rules' coordinate space).
    pub units: Vec<Unit>,
    /// slot id -> per-theme color (`None` = this theme leaves it on the
    /// default foreground). Indexing: `slots[slot][theme]`.
    pub slots: Vec<Vec<Option<Color>>>,
    /// Default foreground color per theme (palette fallback color).
    pub theme_fgs: Vec<Color>,
    /// Glyph count of the source font (alt glyph ids start here).
    pub num_source_glyphs: u16,
    /// Plain (uncolored) glyphs appended after the alt copies, each with a
    /// cmap mapping. Currently always empty: browsers shape each source
    /// line as its own run, so an injected newline glyph cannot keep a
    /// run whole and generic mode maps none (a `\n` is a hard barrier,
    /// like any unmapped char). When non-empty, the output cmap is
    /// resynthesized; when empty, the source cmap is copied byte-for-byte.
    pub plain_glyphs: Vec<(u32, u16)>,
    /// Per-block cell ranges into the concatenated cell vector.
    pub block_ranges: Vec<(usize, usize)>,
    /// Number of cells not colored in any theme.
    pub uncolored_cells: usize,
    /// Non-fatal degradations (tokens with no glyphs, unresolvable contexts).
    pub warnings: Vec<String>,
}

/// A block whose text has already been tokenized and colorized.
pub struct PreparedBlock<'a> {
    pub id: String,
    pub text: &'a str,
    /// `colors[theme][cell]` — resolved color per character per theme.
    pub colors: Vec<Vec<Option<Color>>>,
}

/// HarfBuzz/rustybuzz never apply a contextual rule whose total window
/// (backtrack + input + lookahead) exceeds this many glyphs.
const SHAPER_MAX_CONTEXT: usize = 64;
/// Long occurrences (comments, strings) are colored by several chained
/// segment rules of at most this many input glyphs, leaving room for context.
const SEGMENT_MAX_INPUT: usize = 48;

/// Compile colorized blocks into a glyph plan.
///
/// `charmap` resolves chars to base glyph ids; when the next char is a
/// variation selector it is passed along so the closure can resolve the
/// sequence through the font's cmap14 (UVS). `num_source_glyphs` is where
/// alt glyph ids start; `theme_fgs` backs the CPAL palette fallback color
/// (one entry per theme, parallel to `PreparedBlock::colors`).
///
/// `shaper` provides build-time shaping ground truth per block text (exact
/// mode with ligatures on): the glyphs the source font's own shaping
/// produces, aligned by cluster. `None` keeps the classic per-character
/// model with no ligature restoration.
pub fn compile(
    blocks: &[PreparedBlock<'_>],
    charmap: &dyn Fn(char, Option<char>) -> Option<u16>,
    num_source_glyphs: u16,
    theme_fgs: &[Color],
    shaper: Option<Shaper<'_>>,
) -> Result<GlyphPlan, BuildError> {
    let num_themes = blocks
        .first()
        .map(|b| b.colors.len())
        .unwrap_or(theme_fgs.len());
    for block in blocks {
        if block.colors.len() != num_themes {
            return Err(BuildError::Tokenize(format!(
                "block '{}': has {} theme color rows, expected {num_themes}",
                block.id,
                block.colors.len()
            )));
        }
    }
    if theme_fgs.len() != num_themes {
        return Err(BuildError::Tokenize(format!(
            "got {} theme foregrounds for {num_themes} themes",
            theme_fgs.len()
        )));
    }

    // ---- slots: distinct per-theme color vectors -------------------------
    let mut slot_ids: BTreeMap<Vec<Option<Color>>, u16> = BTreeMap::new();
    let mut slots: Vec<Vec<Option<Color>>> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // ---- flatten blocks into cells ---------------------------------------
    let mut cells: Vec<Cell> = Vec::new();
    let mut block_ranges: Vec<(usize, usize)> = Vec::new();
    for (block_idx, block) in blocks.iter().enumerate() {
        let start = cells.len();
        for (byte, ch) in block.text.char_indices() {
            let cell_idx = cells.len() - start;
            let vector: Vec<Option<Color>> =
                (0..num_themes).map(|t| block.colors[t][cell_idx]).collect();
            let pal = if vector.iter().all(|c| c.is_none()) {
                None
            } else {
                let next = slot_ids.len() as u16;
                Some(*slot_ids.entry(vector.clone()).or_insert_with(|| {
                    slots.push(vector);
                    next
                }))
            };
            // a following variation selector may remap this char through
            // the font's cmap14 (UVS); hidden controls themselves produce
            // no glyph — the shaper drops them
            let next_vs = block
                .text
                .get(byte + ch.len_utf8()..)
                .and_then(|rest| rest.chars().next())
                .filter(|&next| is_variation_selector(next));
            let gid = if is_shaper_hidden(ch) {
                None
            } else {
                charmap(ch, next_vs)
            };
            cells.push(Cell {
                byte_start: byte,
                ch,
                gid,
                block: block_idx,
                pal,
            });
        }
        let end = cells.len();
        if end - start != block.text.chars().count() {
            return Err(BuildError::Tokenize(format!(
                "block '{}': {} color rows vs {} chars",
                block.id,
                end - start,
                block.text.chars().count()
            )));
        }
        block_ranges.push((start, end));
    }

    // ---- barriers: unmapped chars cannot be colored ----------------------
    // A char with no cmap entry and no shaper-side hiding is a barrier: it
    // shapes to .notdef here, and in a browser it moves to a fallback font,
    // breaking the shaping run. Grammars often scope such chars into tokens
    // (a trailing newline in a line comment, an emoji inside a string), but
    // since they produce no colorable glyph the cells simply stay on the
    // base foreground and coloring continues around them.
    {
        let mut unmapped = 0usize;
        let mut missing: Vec<char> = Vec::new();
        let mut missing_seen: HashSet<char> = HashSet::new();
        for cell in cells.iter_mut() {
            if cell.gid.is_none() && !is_shaper_hidden(cell.ch) && cell.pal.is_some() {
                cell.pal = None;
                unmapped += 1;
                if missing_seen.insert(cell.ch) {
                    missing.push(cell.ch);
                }
            } else if is_run_split(cell.ch) && cell.pal.is_some() {
                // run-splitting chars carry no visible ink; keeping them
                // uncolored is visually identical and simplifies the model
                cell.pal = None;
            }
        }
        if unmapped > 0 {
            // name the actual characters so "why is my comment not
            // highlighted?" answers itself in the build log
            let display: String = missing
                .iter()
                .take(16)
                .map(|&c| {
                    if c.is_control() {
                        format!("U+{:04X} ", c as u32)
                    } else {
                        c.to_string()
                    }
                })
                .collect();
            let ellipsis = if missing.len() > 16 { "…" } else { "" };
            warnings.push(format!(
                "{unmapped} colored character(s) have no glyph in the font — those \
                 chars render via the browser's fallback font and stay uncolored \
                 (coloring continues around them). Missing: {display}{ellipsis}",
            ));
            let cjk = missing.iter().any(|&c| {
                matches!(c as u32,
                    0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x2FA1F)
            });
            if cjk {
                warnings.push(
                    "the text includes CJK characters the font does not cover — they will \
                     render in the fallback font and cannot be highlighted; use a CJK-capable \
                     monospace font (e.g. Sarasa Mono SC) to color them"
                        .into(),
                );
            }
        }
    }

    // ---- shaping ground truth: protos + substitution sites ---------------
    // `Proto` narrates the pristine stream once per glyph-producing group:
    // a plain mapped cell, a re-materialized site, or a separator (unmapped
    // char / block boundary — `-1` in both streams).
    let mut mixed_sites = 0usize;
    let mut dropped_sites = 0usize;
    let mut misaligned_blocks = 0usize;
    let mut sites: Vec<Site> = Vec::new();
    let mut protos: Vec<Proto> = Vec::new();
    for (block_idx, &(lo, hi)) in block_ranges.iter().enumerate() {
        let groups_before = protos.len();
        let sites_before = sites.len();
        // empty blocks carry no protos of their own — just the boundary
        let aligned = lo >= hi
            || shaper.is_some_and(|shape| {
                align_shaping(
                    shape(blocks[block_idx].text),
                    &cells[lo..hi],
                    lo,
                    &mut protos,
                    &mut sites,
                    &mut mixed_sites,
                    &mut dropped_sites,
                )
            });
        if !aligned {
            // rustybuzz and the per-char model disagree on this block
            // (merged clusters, bidi reordering, glyph count mismatch):
            // fall back to the classic per-character view
            misaligned_blocks += 1;
            protos.truncate(groups_before);
            sites.truncate(sites_before);
            classic_protos(lo, hi, &cells, &mut protos);
        }
    }
    if shaper.is_some() && misaligned_blocks > 0 {
        warnings.push(format!(
            "{misaligned_blocks} block(s) could not be aligned with the source font's \
             shaping; ligatures are not restored there",
        ));
    }
    if mixed_sites > 0 {
        warnings.push(format!(
            "{mixed_sites} ligature site(s) span multiple colors — rendered without \
             the font's ligature so every character keeps its color",
        ));
    }
    if dropped_sites > 0 {
        warnings.push(format!(
            "{dropped_sites} substitution site(s) could not be re-materialized \
             (decomposed or crossing unmapped chars) — the characters render with \
             their base glyphs",
        ));
    }

    // ---- pristine stream (`seq`) + site spans ----------------------------
    let mut seq: Vec<i32> = Vec::new();
    for proto in &protos {
        match *proto {
            Proto::Plain(i) => seq.push(i32::from(cells[i].gid.unwrap())),
            Proto::Site(si) => {
                sites[si].s = seq.len();
                for &mcell in &sites[si].members {
                    seq.push(i32::from(cells[mcell].gid.unwrap()));
                }
                sites[si].e = seq.len();
            }
            Proto::Sep => seq.push(-1),
        }
    }

    // ---- ligature fixed point --------------------------------------------
    // Degrading an unresolvable site leaves its members pristine, which
    // changes the restored stream other sites' backtracks see — so the
    // audit re-runs until stable. Sequence matches over `seq` never change,
    // so candidates are computed once and reused across iterations.
    let matcher0 = Matcher::new(seq.clone());
    let mut lig_cand_cache: HashMap<(usize, usize), Rc<Vec<u32>>> = HashMap::new();
    let mut active: Vec<bool> = vec![true; sites.len()];
    loop {
        let (rseq, _units, rpos, _unit_at) = materialize(&cells, &protos, &sites, &active);
        let site_start: HashMap<usize, usize> = sites
            .iter()
            .enumerate()
            .filter(|&(si, _)| active[si])
            .map(|(si, st)| (st.s, si))
            .collect();
        let mut failures: Vec<usize> = Vec::new();
        for (si, _) in sites.iter().enumerate() {
            if !active[si] {
                continue;
            }
            let resolved = build_lig_rule(
                &seq,
                &rseq,
                &rpos,
                &matcher0,
                &mut lig_cand_cache,
                &sites,
                &site_start,
                si,
            )
            .is_some();
            if !resolved {
                failures.push(si);
            }
        }
        if failures.is_empty() {
            break;
        }
        for &si in &failures {
            active[si] = false;
            let st = &sites[si];
            let text: String = cells[st.cell_lo..st.cell_hi].iter().map(|c| c.ch).collect();
            warnings.push(format!(
                "block '{}': ligature {:?} ({} cells) cannot be uniquely \
                 distinguished within the 64-glyph shaper context; rendered without \
                 the font's ligature",
                blocks[cells[st.cell_lo].block].id,
                truncate(&text, 40),
                st.cell_hi - st.cell_lo
            ));
        }
    }

    // final restored stream + units
    let (rseq, mut units, rpos, unit_at) = materialize(&cells, &protos, &sites, &active);

    // ---- classes: one class per distinct glyph ---------------------------
    // Covers pristine glyphs (member inputs, plain cells) and restored
    // glyphs (ligature outputs). Class ids are 1:1 with glyphs in exact
    // mode, so class-level matching at run time equals glyph equality at
    // build time; alt copies join their base's class later (invariant 1).
    let mut base_gids: Vec<u16> = cells.iter().map(|c| c.gid.unwrap_or(0)).collect();
    base_gids.extend(rseq.iter().filter_map(|&g| u16::try_from(g).ok()));
    base_gids.sort_unstable();
    base_gids.dedup();
    let mut class_of: HashMap<u16, u16> = HashMap::with_capacity(base_gids.len() * 2);
    for (class, gid) in base_gids.iter().enumerate() {
        class_of.insert(*gid, (class + 1) as u16);
    }

    // ---- ligature rules (class space) + deduped site table ----------------
    let mut lig_rules: Vec<LigRule> = Vec::new();
    let mut lig_sites: Vec<LigSite> = Vec::new();
    let mut site_table: HashMap<(Vec<u16>, u16), u16> = HashMap::new();
    let site_start: HashMap<usize, usize> = sites
        .iter()
        .enumerate()
        .filter(|&(si, _)| active[si])
        .map(|(si, st)| (st.s, si))
        .collect();
    for (si, st) in sites.iter().enumerate() {
        if !active[si] {
            continue;
        }
        let Some((backtrack, input, lookahead)) = build_lig_rule(
            &seq,
            &rseq,
            &rpos,
            &matcher0,
            &mut lig_cand_cache,
            &sites,
            &site_start,
            si,
        ) else {
            continue; // degraded between the audit and here; impossible today
        };
        let components: Vec<u16> = input.iter().map(|&g| g as u16).collect();
        let key = (components.clone(), st.out);
        let site_idx = *site_table.entry(key.clone()).or_insert_with(|| {
            lig_sites.push(LigSite {
                components,
                out: st.out,
            });
            (lig_sites.len() - 1) as u16
        });
        lig_rules.push(LigRule {
            backtrack: backtrack.iter().map(|&g| class_of[&(g as u16)]).collect(),
            input: input.iter().map(|&g| class_of[&(g as u16)]).collect(),
            lookahead: lookahead.iter().map(|&g| class_of[&(g as u16)]).collect(),
            site: site_idx,
        });
    }
    let mut seen = HashSet::new();
    lig_rules.retain(|rule| {
        seen.insert((
            rule.backtrack.clone(),
            rule.input.clone(),
            rule.lookahead.clone(),
            rule.site,
        ))
    });
    // longest first: when two rules share a first class and could both match
    // at a position, the longer one wins (maximal munch), e.g. `===` before `==`
    lig_rules.sort_by_key(|rule| std::cmp::Reverse(rule.input.len()));

    // ---- global match index over the restored stream ----------------------
    let matcher = Matcher::new(rseq.clone());

    // ---- occurrences: maximal runs of uniformly colored units ------------
    let mut occurrences: Vec<(usize, usize, u16)> = Vec::new(); // unit index spans
    {
        let mut i = 0;
        while i < units.len() {
            if let Some(pal) = units[i].pal {
                let start = i;
                let block = units[i].block;
                while i < units.len()
                    && units[i].pal == Some(pal)
                    && units[i].block == block
                    && (i == start || unit_rpos(&units, i) == unit_rpos(&units, i - 1) + 1)
                {
                    i += 1;
                }
                occurrences.push((start, i, pal));
            } else {
                i += 1;
            }
        }
    }

    // ---- minimal distinguishing contexts + global audit -----------------
    // Unresolvable occurrences degrade to uncolored + warning instead of
    // failing the build; degrading can resolve other occurrences' conflicts,
    // so the loop runs to a fixed point. Sequence matches never change
    // during the loop (only intended slots do), so each segment's candidate
    // positions are computed once and reused across iterations.
    let mut cand_cache: HashMap<(usize, usize), Rc<Vec<u32>>> = HashMap::new();
    let mut active_occ: Vec<(usize, usize, u16)> = occurrences;
    loop {
        let mut failures: Vec<usize> = Vec::new();
        for (i, &(ustart, uend, pal)) in active_occ.iter().enumerate() {
            let s = unit_rpos(&units, ustart);
            let e = unit_rpos(&units, uend - 1) + 1;
            let unresolvable = segments(s, e).any(|(ss, se)| {
                let cands = candidates(&matcher, &mut cand_cache, ss, se);
                build_rule(
                    &rseq, &cells, &units, &unit_at, &class_of, ss, se, pal, &cands,
                )
                .is_none()
            });
            if unresolvable {
                failures.push(i);
            }
        }
        if failures.is_empty() {
            break;
        }
        for &i in &failures {
            let (ustart, uend, _pal) = active_occ[i];
            let lo = units[ustart].cell as usize;
            let hi = (units[uend - 1].cell + units[uend - 1].len) as usize;
            let text: String = cells[lo..hi].iter().map(|c| c.ch).collect();
            warnings.push(format!(
                "block '{}': occurrence of {:?} ({} cells) cannot be uniquely \
                 distinguished within the 64-glyph shaper context; left uncolored",
                blocks[units[ustart].block].id,
                truncate(&text, 40),
                hi - lo
            ));
            for cell in &mut cells[lo..hi] {
                cell.pal = None;
            }
        }
        for &i in failures.iter().rev() {
            active_occ.swap_remove(i);
        }
    }
    let mut rules: Vec<Rule> = Vec::new();
    for &(ustart, uend, pal) in &active_occ {
        let s = unit_rpos(&units, ustart);
        let e = unit_rpos(&units, uend - 1) + 1;
        for (ss, se) in segments(s, e) {
            let cands = candidates(&matcher, &mut cand_cache, ss, se);
            if let Some(rule) = build_rule(
                &rseq, &cells, &units, &unit_at, &class_of, ss, se, pal, &cands,
            ) {
                rules.push(rule);
            }
        }
    }

    // ---- dedupe identical rules ------------------------------------------
    let mut seen = HashSet::new();
    rules.retain(|rule| {
        seen.insert((
            rule.backtrack.clone(),
            rule.input.clone(),
            rule.lookahead.clone(),
            rule.pal,
        ))
    });

    // Sort by input length descending: when two rules share a first class and
    // could both match at a position, the longer one wins (maximal munch),
    // e.g. `function` before `fun`.
    rules.sort_by_key(|rule| std::cmp::Reverse(rule.input.len()));

    // ---- alt glyphs: one colored copy per (glyph, slot) ------------------
    let mut pairs: HashSet<(u16, u16)> = HashSet::new();
    for &(ustart, uend, pal) in &active_occ {
        let s = unit_rpos(&units, ustart);
        let e = unit_rpos(&units, uend - 1) + 1;
        for &g in &rseq[s..e] {
            pairs.insert((g as u16, pal));
        }
    }
    let mut pairs: Vec<(u16, u16)> = pairs.into_iter().collect();
    pairs.sort_unstable();
    let alt_glyphs: Vec<(u16, u16, u16)> = pairs
        .iter()
        .enumerate()
        .map(|(i, &(base, pal))| (base, pal, num_source_glyphs + i as u16))
        .collect();
    for &(base, _pal, alt) in &alt_glyphs {
        let class = class_of[&base];
        class_of.insert(alt, class);
    }

    // ---- unit slots reflect the final (possibly degraded) cell colors ----
    for unit in units.iter_mut() {
        let lo = unit.cell as usize;
        let hi = lo + unit.len as usize;
        let mut uniform = cells[lo].pal;
        for cell in &cells[lo + 1..hi] {
            if cell.pal != uniform {
                uniform = None;
                break;
            }
        }
        unit.pal = uniform;
    }
    let uncolored_cells = cells.iter().filter(|c| c.pal.is_none()).count();

    Ok(GlyphPlan {
        cells,
        alt_glyphs,
        class_of,
        rules,
        lig_rules,
        lig_sites,
        units,
        slots,
        theme_fgs: theme_fgs.to_vec(),
        num_source_glyphs,
        plain_glyphs: Vec::new(),
        block_ranges,
        uncolored_cells,
        warnings,
    })
}

/// One re-materialized substitution site, between the alignment walk and
/// the rule builders.
struct Site {
    /// Member cells (mapped), in order; hidden controls may sit between them.
    members: Vec<usize>,
    /// `[cell_lo, cell_hi)` — cells covered by the restored glyph (from the
    /// first member to the last, trailing hidden controls excluded).
    cell_lo: usize,
    cell_hi: usize,
    /// Shaped output glyph.
    out: u16,
    /// Uniform slot across the covered cells (mixed sites are never kept).
    pal: Option<u16>,
    /// `[s, e)` span in the pristine stream, filled when `seq` is built.
    s: usize,
    e: usize,
}

/// One glyph-producing group of the pristine stream.
enum Proto {
    /// A plain mapped cell producing its cmap glyph.
    Plain(usize),
    /// A re-materialized substitution site (index into `sites`).
    Site(usize),
    /// An unmapped char or a block boundary: `-1` in both streams.
    Sep,
}

/// The classic per-character view used without a shaper (and as the
/// degradation fallback for blocks the shaper could not align).
fn classic_protos(lo: usize, hi: usize, cells: &[Cell], protos: &mut Vec<Proto>) {
    for (i, cell) in cells.iter().enumerate().take(hi).skip(lo) {
        if is_shaper_hidden(cell.ch) {
            continue;
        }
        protos.push(if cell.gid.is_some() && !is_run_split(cell.ch) {
            Proto::Plain(i)
        } else {
            Proto::Sep
        });
    }
    protos.push(Proto::Sep); // block boundary
}

/// Walk the shaping ground truth of one block against its cells and emit
/// protos (and substitution sites). `cells` is the block's slice with
/// LOCAL indices; every proto and site records GLOBAL cell indices
/// (offset by `lo`). Returns `false` when the two views cannot be aligned
/// (glyph count or cluster mismatch — e.g. bidi reordering); the caller
/// then degrades the block to the classic view.
#[allow(clippy::too_many_arguments)]
fn align_shaping(
    infos: Vec<ShapedGlyph>,
    cells: &[Cell],
    lo: usize,
    protos: &mut Vec<Proto>,
    sites: &mut Vec<Site>,
    mixed_sites: &mut usize,
    dropped_sites: &mut usize,
) -> bool {
    let nonhidden: Vec<usize> = (0..cells.len())
        .filter(|&i| !is_shaper_hidden(cells[i].ch))
        .collect();
    let mut ci = 0; // index into `nonhidden` (expected next cell)
    let mut i = 0; // index into `infos`
    while i < infos.len() {
        let cluster = infos[i].cluster as usize;
        let Some(&cell) = nonhidden.get(ci) else {
            return false; // more glyphs than chars left
        };
        if cluster < cells[cell].byte_start {
            // artifact of a hidden control (own or merged cluster)
            i += 1;
            continue;
        }
        if cluster > cells[cell].byte_start {
            return false; // a char produced no glyph / clusters not monotone
        }
        // one or more glyphs share this cluster (MultipleSubst degrades)
        let glyphs: Vec<u16> = infos[i..]
            .iter()
            .take_while(|g| g.cluster as usize == cluster)
            .map(|g| g.glyph)
            .collect();
        let j = i + glyphs.len();
        // span: cells from `cell` until the next glyph's cluster
        let next_cluster = infos
            .get(j)
            .map(|g| g.cluster as usize)
            .unwrap_or(usize::MAX);
        let span_end = nonhidden[ci..]
            .iter()
            .position(|&c| cells[c].byte_start >= next_cluster)
            .map_or(nonhidden.len(), |p| ci + p);
        let cell_lo = nonhidden[ci];
        let cell_hi = if span_end < nonhidden.len() {
            nonhidden[span_end]
        } else {
            cells.len()
        };
        let members: Vec<usize> = (cell_lo..cell_hi)
            .filter(|&c| cells[c].gid.is_some())
            .map(|c| c + lo)
            .collect();
        let has_barrier = (cell_lo..cell_hi).any(|c| cells[c].gid.is_none());

        // a run-splitting char shapes into its own run: a barrier, never
        // a site and never a dropped-site warning
        if glyphs.len() == 1 && members.len() == 1 && is_run_split(cells[members[0] - lo].ch) {
            protos.push(Proto::Sep);
            ci = span_end;
            i = j;
            continue;
        }
        let splits_run = (cell_lo..cell_hi).any(|c| is_run_split(cells[c].ch));
        if glyphs.len() == 1 && !members.is_empty() && !splits_run {
            let glyph = glyphs[0];
            if members.len() == 1 && cells[members[0] - lo].gid == Some(glyph) {
                // the shaper produced exactly the cmap glyph: plain cell
                protos.push(Proto::Plain(members[0]));
            } else if !has_barrier && glyph != 0 {
                // a real substitution site: keep it iff the covered cells
                // color uniformly, else degrade so colors win
                let first_pal = cells[members[0] - lo].pal;
                if members.iter().any(|&m| cells[m - lo].pal != first_pal) {
                    *mixed_sites += 1;
                    push_classic_span(cells, lo, cell_lo, cell_hi, protos);
                } else {
                    sites.push(Site {
                        members: members.clone(),
                        cell_lo: members[0],
                        cell_hi: members[members.len() - 1] + 1,
                        out: glyph,
                        pal: first_pal,
                        s: 0,
                        e: 0,
                    });
                    protos.push(Proto::Site(sites.len() - 1));
                }
            } else {
                // ligating across an unmapped char never happens in a
                // browser (fallback splits the run first), and a ligature
                // to .notdef is nonsense
                *dropped_sites += 1;
                push_classic_span(cells, lo, cell_lo, cell_hi, protos);
            }
        } else if !members.is_empty() {
            // 1:N decomposition: not expressible as a restore rule
            *dropped_sites += 1;
            push_classic_span(cells, lo, cell_lo, cell_hi, protos);
        } else {
            // unmapped run: shapes to .notdef here, moves to a fallback
            // font in the browser — a separator either way
            protos.push(Proto::Sep);
        }
        ci = span_end;
        i = j;
    }
    if ci < nonhidden.len() {
        return false; // chars left over: the shaper dropped mapped glyphs
    }
    protos.push(Proto::Sep); // block boundary
    true
}

/// Re-emit one alignment span as the classic per-character view.
fn push_classic_span(
    cells: &[Cell],
    lo: usize,
    cell_lo: usize,
    cell_hi: usize,
    protos: &mut Vec<Proto>,
) {
    for (c, cell) in cells.iter().enumerate().take(cell_hi).skip(cell_lo) {
        if is_shaper_hidden(cell.ch) {
            continue;
        }
        protos.push(if cell.gid.is_some() && !is_run_split(cell.ch) {
            Proto::Plain(c + lo)
        } else {
            Proto::Sep
        });
    }
}

/// Build the restored stream (`rseq`), the unit list, the pristine→restored
/// position map (`rpos`: seq index → rseq index, `-1` when the position is
/// consumed by a site), and the rseq-position → unit-index map.
fn materialize(
    cells: &[Cell],
    protos: &[Proto],
    sites: &[Site],
    active: &[bool],
) -> (Vec<i32>, Vec<Unit>, Vec<i32>, Vec<usize>) {
    let mut rseq: Vec<i32> = Vec::new();
    let mut units: Vec<Unit> = Vec::new();
    let mut rpos: Vec<i32> = Vec::new();
    let mut unit_at: Vec<usize> = Vec::new();
    for proto in protos {
        match *proto {
            Proto::Plain(i) => {
                rpos.push(rseq.len() as i32);
                unit_at.push(units.len());
                let cell = &cells[i];
                units.push(Unit {
                    cell: i as u32,
                    len: 1,
                    glyph: cell.gid.unwrap(),
                    pal: cell.pal,
                    block: cell.block,
                    rpos: rseq.len() as u32,
                });
                rseq.push(i32::from(cell.gid.unwrap()));
            }
            Proto::Site(si) => {
                let st = &sites[si];
                if active[si] {
                    rpos.push(rseq.len() as i32);
                    for _ in st.s + 1..st.e {
                        rpos.push(-1);
                    }
                    unit_at.push(units.len());
                    units.push(Unit {
                        cell: st.cell_lo as u32,
                        len: (st.cell_hi - st.cell_lo) as u32,
                        glyph: st.out,
                        pal: st.pal,
                        block: cells[st.cell_lo].block,
                        rpos: rseq.len() as u32,
                    });
                    rseq.push(i32::from(st.out));
                } else {
                    for &mcell in &st.members {
                        rpos.push(rseq.len() as i32);
                        unit_at.push(units.len());
                        let cell = &cells[mcell];
                        units.push(Unit {
                            cell: mcell as u32,
                            len: 1,
                            glyph: cell.gid.unwrap(),
                            pal: cell.pal,
                            block: cell.block,
                            rpos: rseq.len() as u32,
                        });
                        rseq.push(i32::from(cell.gid.unwrap()));
                    }
                }
            }
            Proto::Sep => {
                rpos.push(-1);
                unit_at.push(usize::MAX);
                rseq.push(-1);
            }
        }
    }
    (rseq, units, rpos, unit_at)
}

/// rseq position of a unit (units are in stream order, but consecutive
/// units are only stream-adjacent when their rpos values differ by one —
/// separators sit between them otherwise).
fn unit_rpos(units: &[Unit], i: usize) -> usize {
    units[i].rpos as usize
}

/// Split `[start, end)` into roughly balanced segments.
fn segments(start: usize, end: usize) -> impl Iterator<Item = (usize, usize)> {
    let m = end - start;
    let count = m.div_ceil(SEGMENT_MAX_INPUT).max(1);
    let seg = m.div_ceil(count);
    (0..m)
        .step_by(seg)
        .map(move |off| (start + off, (start + off + seg).min(end)))
}

/// Chars the shaper hides entirely (produces no glyph for) when the font
/// does not map them: variation selectors plus zero-width/bidi format
/// controls. These are transparent to rule building: they never split an
/// occurrence and occupy no position in the shaping view.
/// Chars the browser's shaper splits shaping runs at (Blink shapes
/// space-delimited sub-runs to apply spacing). No contextual rule can see
/// across them, so they are hard separators for rule contexts — exactly
/// like unmapped chars — and never carry color (they have no visible ink
/// anyway). Verified empirically against Chromium: a chain rule whose
/// backtrack is the space class never fires, while intra-word backtrack
/// does.
pub(crate) fn is_run_split(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\u{2000}'..='\u{200A}' | '\u{3000}')
}

pub(crate) fn is_shaper_hidden(ch: char) -> bool {
    matches!(
        ch,
        '\u{FE00}'..='\u{FE0F}'      // variation selectors 1–16
            | '\u{180B}'..='\u{180F}' // Mongolian selectors + FVS4
            | '\u{E0100}'..='\u{E01EF}' // ideographic selectors
            | '\u{200B}'..='\u{200F}' // ZWSP/ZWNJ/ZWJ/LRM/RLM
    )
}

/// Variation selectors proper — a preceding char may be remapped through
/// the font's cmap14 when followed by one of these.
fn is_variation_selector(ch: char) -> bool {
    matches!(
        ch,
        '\u{FE00}'..='\u{FE0F}' | '\u{180B}'..='\u{180F}' | '\u{E0100}'..='\u{E01EF}'
    )
}

/// Cached candidate positions for the segment `[s, e)` in one matcher's
/// space: every other shaped position carrying the same glyph run
/// (separators keep matches inside one block and one barrier-free run).
/// Sequence matches are independent of the fixed point loops, so this is
/// computed once per segment.
fn candidates(
    matcher: &Matcher,
    cache: &mut HashMap<(usize, usize), Rc<Vec<u32>>>,
    s: usize,
    e: usize,
) -> Rc<Vec<u32>> {
    if let Some(rc) = cache.get(&(s, e)) {
        return rc.clone();
    }
    let rc: Rc<Vec<u32>> = Rc::new(
        matcher
            .matches(s, e - s)
            .iter()
            .copied()
            .filter(|&q| q as usize != s)
            .collect(),
    );
    cache.insert((s, e), rc.clone());
    rc
}

/// Build one contextual coloring rule for the restored-stream segment
/// `[s, e)` in slot `pal`: grow the backtrack/lookahead window minimally
/// until no false match remains, staying inside the shaper's rule budget.
/// `None` if the occurrence cannot be uniquely pinned (caller degrades it).
#[allow(clippy::too_many_arguments)] // independent coordinates of one probe
fn build_rule(
    rseq: &[i32],
    cells: &[Cell],
    units: &[Unit],
    unit_at: &[usize],
    class_of: &HashMap<u16, u16>,
    s: usize,
    e: usize,
    pal: u16,
    cands: &[u32],
) -> Option<Rule> {
    let m = e - s;
    debug_assert!(m < SHAPER_MAX_CONTEXT, "segments leave room for context");
    let budget = SHAPER_MAX_CONTEXT - m; // max backtrack + lookahead combined
                                         // windows stop at separators: contexts never legitimately cross an
                                         // unmapped char (font fallback moves it out of this font's run) or a
                                         // block boundary
    let max_l = (0..s)
        .rev()
        .take_while(|&k| rseq[k] >= 0)
        .count()
        .min(budget);
    let max_r = (e..rseq.len())
        .take_while(|&k| rseq[k] >= 0)
        .count()
        .min(budget);
    let mut l = 0usize;
    let mut r = 0usize;
    let mut grow_left = true;
    loop {
        if audit(rseq, cells, units, unit_at, s, e, pal, l, r, cands).is_some() {
            let can_l = l < max_l && l + r < budget;
            let can_r = r < max_r && l + r < budget;
            if can_l && (grow_left || !can_r) {
                l += 1;
                grow_left = false;
            } else if can_r {
                r += 1;
                grow_left = true;
            } else {
                return None; // unresolvable within the 64-glyph budget
            }
        } else {
            let backtrack = (1..=l).map(|k| class_of[&(rseq[s - k] as u16)]).collect();
            let input = rseq[s..e].iter().map(|&g| class_of[&(g as u16)]).collect();
            let lookahead = (1..=r)
                .map(|k| class_of[&(rseq[e + k - 1] as u16)])
                .collect();
            return Some(Rule {
                backtrack,
                input,
                lookahead,
                pal,
            });
        }
    }
}

/// Check whether a coloring rule for the restored-stream segment `[s, e)`
/// with window `(l, r)` would falsely fire at any candidate position.
/// Returns the first conflicting position (restored index) if so.
///
/// A candidate `q` falsely fires iff its glyph run equals the occurrence's
/// and the `l` glyphs before and `r` glyphs after also match. Comparisons
/// happen in `rseq` space, so a window crossing a separator (`-1`) can
/// never equal a window of real glyph ids. A firing is harmless iff every
/// covered cell — hidden controls inside the span included — already
/// intends this slot.
#[allow(clippy::too_many_arguments)] // independent coordinates of one probe
fn audit(
    rseq: &[i32],
    cells: &[Cell],
    units: &[Unit],
    unit_at: &[usize],
    s: usize,
    e: usize,
    pal: u16,
    l: usize,
    r: usize,
    cands: &[u32],
) -> Option<usize> {
    let m = e - s;

    for &q in cands {
        let q = q as usize;
        // the rule cannot fire where its window does not physically fit
        // (start/end of the whole glyph stream); inside it, separators
        // reject crossings by mere inequality
        if q < l || q + m + r > rseq.len() {
            continue;
        }
        // backtrack equality (a `-1` separator never equals a glyph id)
        let mut equal = true;
        for k in 1..=l {
            if rseq[q - k] != rseq[s - k] {
                equal = false;
                break;
            }
        }
        if !equal {
            continue;
        }
        // lookahead equality
        let mut equal = true;
        for k in 1..=r {
            if rseq[q + m + k - 1] != rseq[e + k - 1] {
                equal = false;
                break;
            }
        }
        if !equal {
            continue;
        }
        // The rule would fire at q. Harmless iff every covered cell —
        // including hidden controls inside the span — is already intended
        // to carry this slot.
        let first = units[unit_at[q]];
        let last = units[unit_at[q + m - 1]];
        let lo = first.cell as usize;
        let hi = (last.cell + last.len) as usize;
        if cells[lo..hi].iter().any(|c| c.pal != Some(pal)) {
            return Some(q);
        }
    }
    None
}

/// Build one contextual ligature-restore rule for site `si` as glyph-level
/// window (backtrack, input, lookahead): grow the window minimally until
/// no false match remains, staying inside the shaper's rule budget.
///
/// Coordinate spaces (see the module docs): input and lookahead are
/// pristine-stream glyphs — the rule runs before any substitution; the
/// backtrack is restored-stream glyphs — earlier rules of the same lookup
/// have already replaced their spans when this one matches.
#[allow(clippy::too_many_arguments)] // independent coordinates of one probe
fn build_lig_rule(
    seq: &[i32],
    rseq: &[i32],
    rpos: &[i32],
    matcher: &Matcher,
    cache: &mut HashMap<(usize, usize), Rc<Vec<u32>>>,
    sites: &[Site],
    site_start: &HashMap<usize, usize>,
    si: usize,
) -> Option<(Vec<i32>, Vec<i32>, Vec<i32>)> {
    let st = &sites[si];
    let s = st.s;
    let e = st.e;
    let m = e - s;
    if m >= SHAPER_MAX_CONTEXT {
        return None; // the input alone busts the shaper's rule budget
    }
    let budget = SHAPER_MAX_CONTEXT - m;
    let rs = rpos[s];
    debug_assert!(rs >= 0, "an active site's first member maps to its unit");
    let rs = rs as usize;
    // backtrack walks the restored stream (earlier rules already fired);
    // lookahead walks the pristine stream (later rules have not fired yet)
    let max_l = (0..rs)
        .rev()
        .take_while(|&k| rseq[k] >= 0)
        .count()
        .min(budget);
    let max_r = (e..seq.len())
        .take_while(|&k| seq[k] >= 0)
        .count()
        .min(budget);
    let cands = candidates(matcher, cache, s, e);
    let mut l = 0usize;
    let mut r = 0usize;
    let mut grow_left = true;
    loop {
        if lig_audit(
            seq, rseq, rpos, s, e, st.out, l, r, rs, &cands, sites, site_start,
        )
        .is_some()
        {
            let can_l = l < max_l && l + r < budget;
            let can_r = r < max_r && l + r < budget;
            if can_l && (grow_left || !can_r) {
                l += 1;
                grow_left = false;
            } else if can_r {
                r += 1;
                grow_left = true;
            } else {
                return None; // unresolvable within the 64-glyph budget
            }
        } else {
            let backtrack = (1..=l).map(|k| rseq[rs - k]).collect();
            let input = seq[s..e].to_vec();
            let lookahead = (1..=r).map(|k| seq[e + k - 1]).collect();
            return Some((backtrack, input, lookahead));
        }
    }
}

/// Check whether a ligature rule for the pristine span `[s, e)` outputting
/// `out` with window `(l, r)` would falsely fire at any candidate.
///
/// A candidate `q` (same glyph run in the pristine stream) falsely fires
/// iff its restored backtrack and pristine lookahead both match and `q` is
/// not itself an active site start with the same output — firing there
/// would replace text the source font left unligated (or ligated
/// differently). Candidates inside a fired site's span are unreachable at
/// run time (the shaper's cursor never rests inside a substitution) and
/// therefore skipped.
#[allow(clippy::too_many_arguments)] // independent coordinates of one probe
fn lig_audit(
    seq: &[i32],
    rseq: &[i32],
    rpos: &[i32],
    s: usize,
    e: usize,
    out: u16,
    l: usize,
    r: usize,
    rs: usize,
    cands: &[u32],
    sites: &[Site],
    site_start: &HashMap<usize, usize>,
) -> Option<usize> {
    let m = e - s;
    for &q0 in cands {
        let q = q0 as usize;
        let rq = rpos[q];
        if rq < 0 {
            continue; // inside a fired site: the cursor never rests there
        }
        let rq = rq as usize;
        if rq < l {
            continue; // the window cannot physically fit at run time
        }
        // backtrack equality in the restored stream
        let mut equal = true;
        for k in 1..=l {
            if rseq[rq - k] != rseq[rs - k] {
                equal = false;
                break;
            }
        }
        if !equal {
            continue;
        }
        // lookahead equality in the pristine stream
        let mut equal = true;
        for k in 1..=r {
            match seq.get(q + m + k - 1) {
                Some(&v) if v == seq[e + k - 1] => continue,
                _ => {
                    equal = false;
                    break;
                }
            }
        }
        if !equal {
            continue;
        }
        // The rule would fire at q. Harmless iff the ground truth there is
        // the same substitution: an active site with the same output.
        match site_start.get(&q) {
            Some(&qi) if sites[qi].out == out => {}
            _ => return Some(q),
        }
    }
    None
}

/// Suffix-array index over the flattened glyph sequence (1:1 with cells;
/// unmapped chars use glyph 0). Answers "at which positions does the glyph
/// sequence starting at `start`, length `m`, also occur?" in O(m log n).
/// The caller filters matches down to those within a single block.
struct Matcher {
    seq: Vec<i32>,
    sa: Vec<u32>,
}

impl Matcher {
    fn new(seq: Vec<i32>) -> Self {
        let n = seq.len();
        let mut sa: Vec<u32> = (0..n as u32).collect();
        if n == 0 {
            return Self { seq, sa };
        }
        // prefix doubling: O(n log² n) with a comparison sort, plenty for
        // build-time inputs in the tens of thousands of cells
        let mut rank: Vec<i32> = seq.clone();
        let mut next: Vec<i32> = vec![0; n];
        let key = |i: usize, rank: &[i32], k: usize| {
            // past-end must sort below every in-sequence value, sentinel
            // included — that is what makes doubling terminate even when one
            // suffix is a prefix of another
            let second = if i + k < n { rank[i + k] } else { -2 };
            (rank[i], second)
        };
        let mut k = 1usize;
        loop {
            sa.sort_unstable_by(|&a, &b| key(a as usize, &rank, k).cmp(&key(b as usize, &rank, k)));
            next[sa[0] as usize] = 0;
            for w in 1..n {
                let (prev, cur) = (sa[w - 1] as usize, sa[w] as usize);
                let same = key(prev, &rank, k) == key(cur, &rank, k);
                next[cur] = next[prev] + u32::from(!same) as i32;
            }
            std::mem::swap(&mut rank, &mut next);
            if rank[sa[n - 1] as usize] == n as i32 - 1 {
                break; // all ranks distinct: the suffix array is total
            }
            k *= 2;
        }
        Self { seq, sa }
    }

    /// Positions whose next `m` glyphs equal `seq[start..start+m]`, as a
    /// slice of suffix-array entries (unordered; includes `start` itself).
    fn matches(&self, start: usize, m: usize) -> &[u32] {
        let cmp_suffix = |suf: usize| -> std::cmp::Ordering {
            for j in 0..m {
                let pv = self.seq[start + j];
                match self.seq.get(suf + j) {
                    Some(&sv) if sv == pv => continue,
                    Some(&sv) => return sv.cmp(&pv),
                    // suffix ran out: it is a proper prefix of the pattern
                    None => return std::cmp::Ordering::Less,
                }
            }
            std::cmp::Ordering::Equal
        };
        let lo = self
            .sa
            .partition_point(|&s| cmp_suffix(s as usize) == std::cmp::Ordering::Less);
        let hi = lo
            + self.sa[lo..]
                .partition_point(|&s| cmp_suffix(s as usize) == std::cmp::Ordering::Equal);
        &self.sa[lo..hi]
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max_chars).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUE: Color = Color::rgb(1, 1, 1);
    const RED: Color = Color::rgb(3, 3, 3);
    type Spans<'a> = &'a [(usize, usize, Option<Color>)];
    type SpanBlocks<'a> = &'a [(&'a str, Spans<'a>)];

    /// Build a plan from (text, per-char color runs): spans is
    /// `[(start_char, end_char, Some(color) | None)]` under one theme.
    fn plan_for(texts: SpanBlocks<'_>) -> GlyphPlan {
        plan_for_themes(texts, &[], &[], None)
    }

    /// Multi-theme variant: `themes` is extra per-theme color runs; theme 0
    /// uses `texts`' spans, theme k uses `themes[k]`' spans.
    fn plan_for_themes(
        texts: SpanBlocks<'_>,
        _extra: &[&[(usize, usize, Option<Color>)]],
        _extra_fgs: &[Color],
        shaped: Option<&[ShapedGlyph]>,
    ) -> GlyphPlan {
        let charmap = |ch: char, _vs: Option<char>| -> Option<u16> {
            match ch {
                'a'..='z' => Some(10 + (ch as u16 - 'a' as u16)),
                ' ' => Some(2),
                _ => Some(3),
            }
        };
        let blocks: Vec<PreparedBlock<'_>> = texts
            .iter()
            .map(|(text, spans)| {
                let n = text.chars().count();
                let mut row = vec![None; n];
                for &(s, e, color) in *spans {
                    for c in row.iter_mut().take(e).skip(s) {
                        *c = color;
                    }
                }
                PreparedBlock {
                    id: "b".into(),
                    text,
                    colors: vec![row],
                }
            })
            .collect();
        let shaper_cb;
        let shaper: Option<Shaper<'_>> = match shaped {
            // every block reports the same shaped stream (tests are
            // single-block unless they say otherwise)
            Some(s) => {
                shaper_cb = move |_text: &str| s.to_vec();
                Some(&shaper_cb)
            }
            None => None,
        };
        compile(
            &blocks,
            &charmap,
            100,
            &[Color::rgb(0xd4, 0xd4, 0xd4)],
            shaper,
        )
        .unwrap()
    }

    #[test]
    fn minimal_context_same_color_shares_rule() {
        // "let" twice with the same color -> a single deduped rule, no context
        let plan = plan_for(&[("let a; let b;", &[(0, 3, Some(BLUE)), (7, 10, Some(BLUE))])]);
        assert_eq!(plan.rules.len(), 1);
        assert!(plan.rules[0].backtrack.is_empty() && plan.rules[0].lookahead.is_empty());
        assert_eq!(plan.alt_glyphs.len(), 3);
    }

    #[test]
    fn same_text_different_color_gets_context() {
        // "ab" blue then "ab" red ADJACENT: the two rules distinguish via
        // windows inside one run (across a space no browser could tell
        // them apart — see space_separated_duplicates_degrade below)
        let plan = plan_for(&[("abab", &[(0, 2, Some(BLUE)), (2, 4, Some(RED))])]);
        assert_eq!(plan.rules.len(), 2);
        assert!(plan
            .rules
            .iter()
            .any(|r| !r.backtrack.is_empty() || !r.lookahead.is_empty()));
        let r0 = &plan.rules[0];
        let r1 = &plan.rules[1];
        assert_ne!(
            (r0.backtrack.clone(), r0.lookahead.clone()),
            (r1.backtrack.clone(), r1.lookahead.clone())
        );
    }

    #[test]
    fn prefix_protection() {
        // "for" blue + "format" red (adjacent — browsers split runs at
        // spaces, so a space-separated pair could not be distinguished and
        // would degrade; see space_separated_duplicates_degrade)
        let plan = plan_for(&[("forformat", &[(0, 3, Some(BLUE)), (3, 10, Some(RED))])]);
        let for_rules: Vec<&Rule> = plan.rules.iter().filter(|r| r.input.len() == 3).collect();
        assert!(!for_rules.is_empty());
        assert!(
            for_rules
                .iter()
                .all(|r| !r.lookahead.is_empty() || !r.backtrack.is_empty()),
            "for rule needs context: {for_rules:#?}"
        );
    }

    #[test]
    fn periodic_text_resolves() {
        let plan = plan_for(&[(
            "ababab",
            &[(0, 2, Some(BLUE)), (2, 4, Some(RED)), (4, 6, Some(BLUE))],
        )]);
        assert_eq!(plan.rules.len(), 3);
    }

    #[test]
    fn cross_block_identical_colors_degrade_not_fail() {
        // same text, different colors in two single-cell blocks: any rule
        // would fire in both, so both degrade with warnings
        let charmap =
            |ch: char, _vs: Option<char>| -> Option<u16> { Some(10 + (ch as u16 - 'a' as u16)) };
        let mk = |color: Option<Color>| PreparedBlock {
            id: "b".into(),
            text: "ab",
            colors: vec![vec![color, color]],
        };
        let blocks = vec![mk(Some(BLUE)), mk(Some(RED))];
        let plan = compile(
            &blocks,
            &charmap,
            100,
            &[Color::rgb(0xd4, 0xd4, 0xd4)],
            None,
        )
        .unwrap();
        assert!(!plan.warnings.is_empty());
        assert!(plan.cells.iter().all(|c| c.pal.is_none()));
        assert!(plan.alt_glyphs.is_empty());
    }

    #[test]
    fn adjacent_same_slot_merges_into_one_rule() {
        // two adjacent same-color runs behave as one occurrence
        let plan = plan_for(&[("abab", &[(0, 2, Some(BLUE)), (2, 4, Some(BLUE))])]);
        assert_eq!(plan.rules.len(), 1);
        assert_eq!(plan.rules[0].input.len(), 4);
    }

    #[test]
    fn long_occurrences_respect_shaper_budget() {
        // 100 same-color cells split into segments; identical segments dedupe
        // into one rule (it fires at every 34-cell window), and every rule
        // stays within the 64-glyph total window budget
        let text: String = "a".repeat(100);
        let n = text.chars().count();
        let plan = plan_for(&[(&text, &[(0, n, Some(BLUE))])]);
        assert!(!plan.rules.is_empty());
        assert!(
            plan.rules
                .iter()
                .all(|r| r.input.len() + r.backtrack.len() + r.lookahead.len() <= 64),
            "{:?}",
            plan.rules
        );
        // enough coverage for the whole run: each rule fires repeatedly
        let max_input = plan.rules.iter().map(|r| r.input.len()).max().unwrap();
        assert!(max_input <= 48);
    }

    #[test]
    fn slots_dedupe_color_vectors() {
        // two themes, same structure -> slots count == distinct color pairs
        let charmap =
            |ch: char, _vs: Option<char>| -> Option<u16> { Some(10 + (ch as u16 - 'a' as u16)) };
        let row = |color: Option<Color>, n: usize| vec![color; n];
        let blocks = vec![PreparedBlock {
            id: "b".into(),
            text: "ab",
            colors: vec![row(Some(BLUE), 2), row(Some(RED), 2)],
        }];
        let plan = compile(&blocks, &charmap, 100, &[BLUE, RED], None).unwrap();
        assert_eq!(plan.slots.len(), 1, "one slot: (blue, red)");
        assert_eq!(plan.slots[0], vec![Some(BLUE), Some(RED)]);
    }

    #[test]
    fn variation_selectors_are_transparent() {
        // "a" + VS16 + "b" in one colored run: the VS has no glyph and must
        // not split the run — one rule covering both sides, no warnings
        let plan = plan_for(&[("a\u{FE0F}b", &[(0, 3, Some(BLUE))])]);
        assert_eq!(plan.warnings.len(), 0);
        assert_eq!(plan.rules.len(), 1);
        assert_eq!(plan.rules[0].input.len(), 2, "input covers a and b only");
        assert_eq!(plan.alt_glyphs.len(), 2);
    }

    #[test]
    fn contexts_never_cross_unmapped_barriers() {
        // "aa" blue | X (no glyph) | "aa" red: distinguishing the two "aa"
        // would require a context across X, which browsers never honor
        // (font fallback moves X out of this font's shaping run) — both
        // occurrences degrade with warnings instead
        let charmap = |ch: char, _vs: Option<char>| -> Option<u16> {
            match ch {
                'a' => Some(10),
                _ => None,
            }
        };
        let blocks = vec![PreparedBlock {
            id: "b".into(),
            text: "aaXaa",
            colors: vec![vec![Some(BLUE), Some(BLUE), None, Some(RED), Some(RED)]],
        }];
        let plan = compile(
            &blocks,
            &charmap,
            100,
            &[Color::rgb(0xd4, 0xd4, 0xd4)],
            None,
        )
        .unwrap();
        assert!(!plan.warnings.is_empty());
        assert!(
            plan.cells
                .iter()
                .all(|c| c.pal.is_none() || c.gid.is_none()),
            "ambiguous occurrences must degrade"
        );
        assert!(plan.rules.is_empty());
    }

    #[test]
    fn uvs_sequence_uses_variant_glyph() {
        // base + VS16 resolved through cmap14: the base cell carries the
        // variant glyph id and the VS itself is transparent
        let charmap = |ch: char, vs: Option<char>| -> Option<u16> {
            match (ch, vs) {
                ('x', Some('\u{FE0F}')) => Some(42), // the UVS glyph
                (c, _) => match c {
                    'a'..='z' => Some(10 + (c as u16 - 'a' as u16)),
                    _ => Some(3),
                },
            }
        };
        let blocks = vec![PreparedBlock {
            id: "b".into(),
            text: "x\u{FE0F}",
            colors: vec![vec![Some(BLUE), Some(BLUE)]],
        }];
        let plan = compile(
            &blocks,
            &charmap,
            100,
            &[Color::rgb(0xd4, 0xd4, 0xd4)],
            None,
        )
        .unwrap();
        assert_eq!(plan.cells[0].gid, Some(42));
        assert_eq!(plan.alt_glyphs.len(), 1);
        assert_eq!(plan.alt_glyphs[0].0, 42, "colored copy of the UVS glyph");
        assert_eq!(plan.rules.len(), 1);
        assert_eq!(plan.rules[0].input.len(), 1);
    }

    // ---- ligature re-materialization --------------------------------------

    fn sg(glyph: u16, cluster: u32) -> ShapedGlyph {
        ShapedGlyph { glyph, cluster }
    }

    #[test]
    fn uniform_ligature_becomes_colored_site() {
        // "ab" colors blue and the font ligates it to glyph 90: one lig
        // rule, one unit covering both cells, and a colored copy of 90
        let plan = plan_for_themes(
            &[("ab", &[(0, 2, Some(BLUE))])],
            &[],
            &[],
            Some(&[sg(90, 0)]),
        );
        assert_eq!(plan.warnings, Vec::<String>::new());
        assert_eq!(plan.lig_rules.len(), 1);
        assert_eq!(plan.lig_rules[0].input.len(), 2);
        assert!(plan.lig_rules[0].backtrack.is_empty());
        assert!(plan.lig_rules[0].lookahead.is_empty());
        assert_eq!(
            plan.lig_sites,
            vec![LigSite {
                components: vec![10, 11],
                out: 90
            }]
        );
        assert_eq!(plan.units.len(), 1);
        assert_eq!(plan.units[0].glyph, 90);
        assert_eq!(plan.units[0].len, 2);
        assert!(plan.units[0].pal.is_some());
        // the ligature glyph itself gets the colored copy
        assert_eq!(plan.alt_glyphs.len(), 1);
        assert_eq!(plan.alt_glyphs[0].0, 90);
        // and the coloring rule targets the ligature glyph's class
        assert_eq!(plan.rules.len(), 1);
        assert_eq!(plan.rules[0].input.len(), 1);
    }

    #[test]
    fn mixed_color_ligature_degrades_to_per_char() {
        // "ab" ligates to 90, but only "a" colors: the ligature is dropped
        // so "a" keeps its color; "a" and "b" stay per-char units
        let plan = plan_for_themes(
            &[("ab", &[(0, 1, Some(BLUE))])],
            &[],
            &[],
            Some(&[sg(90, 0)]),
        );
        assert!(plan.lig_rules.is_empty());
        assert!(plan.lig_sites.is_empty());
        assert_eq!(plan.units.len(), 2);
        assert_eq!(plan.units[0].glyph, 10);
        assert_eq!(plan.units[0].pal, Some(0));
        assert_eq!(plan.units[1].pal, None);
        assert_eq!(plan.alt_glyphs.len(), 1);
        assert_eq!(plan.alt_glyphs[0].0, 10);
        assert!(plan.warnings.iter().any(|w| w.contains("ligature")));
    }

    #[test]
    fn uncolored_ligature_is_restored_without_color() {
        let plan = plan_for_themes(&[("ab", &[])], &[], &[], Some(&[sg(90, 0)]));
        assert_eq!(plan.lig_rules.len(), 1);
        assert_eq!(plan.units.len(), 1);
        assert_eq!(plan.units[0].glyph, 90);
        assert_eq!(plan.units[0].pal, None);
        assert!(plan.alt_glyphs.is_empty());
        assert!(plan.rules.is_empty());
    }

    #[test]
    fn identical_plain_run_gets_ligature_context() {
        // "ab" ligates at 0 but not at 2 (contextual calt): the lig rule
        // must grow a window so it does not fire over the plain "ab"
        // (adjacent — a space would split the runs in a browser)
        let plan = plan_for_themes(
            &[("abab", &[(0, 2, Some(BLUE)), (2, 4, Some(BLUE))])],
            &[],
            &[],
            Some(&[sg(90, 0), sg(10, 2), sg(11, 3)]),
        );
        assert_eq!(plan.lig_rules.len(), 1);
        let rule = &plan.lig_rules[0];
        assert!(
            !rule.lookahead.is_empty() || !rule.backtrack.is_empty(),
            "needs a distinguishing window: {rule:?}"
        );
        // the coloring rules now reference the ligature glyph (unit 0) and
        // the plain a/b units separately — the space glyph breaks the run
        assert_eq!(plan.units.len(), 3);
        assert_eq!(plan.units[0].glyph, 90);
        assert_eq!(plan.units[1].glyph, 10);
    }

    #[test]
    fn space_separated_duplicates_degrade() {
        // browsers split shaping runs at spaces: no rule context can span
        // one, so identical runs on both sides wanting different colors
        // cannot be told apart — both degrade with warnings instead of
        // misfiring in half the browsers
        let plan = plan_for(&[("ab ab", &[(0, 2, Some(BLUE)), (3, 5, Some(RED))])]);
        assert!(!plan.warnings.is_empty());
        assert!(
            plan.cells
                .iter()
                .all(|c| c.pal.is_none() || is_run_split(c.ch)),
            "ambiguous occurrences must degrade"
        );
        assert!(plan.rules.is_empty());
    }

    #[test]
    fn run_splits_are_barriers_and_uncolored() {
        // a colored string containing a space: the space stays uncolored
        // (no visible ink) and the two halves anchor independently — the
        // second half's rule carries no backtrack and still fires at its
        // run start in a browser
        let plan = plan_for(&[("\"a b\"", &[(0, 5, Some(BLUE))])]);
        assert!(plan
            .cells
            .iter()
            .all(|c| is_run_split(c.ch) || c.pal == Some(0)));
        let space = plan.cells.iter().find(|c| c.ch == ' ').unwrap();
        assert_eq!(space.pal, None);
        // both words carry rules with empty backtrack
        assert!(
            plan.rules.iter().all(|r| r.backtrack.is_empty()),
            "{:?}",
            plan.rules
        );
        assert!(plan.rules.len() >= 2, "{:?}", plan.rules);
    }

    #[test]
    fn single_substitution_site_restores_one_to_one() {
        // "a" alone shapes to a contextual alternate 55: a 1:1 restore
        let plan = plan_for_themes(
            &[("a", &[(0, 1, Some(BLUE))])],
            &[],
            &[],
            Some(&[sg(55, 0)]),
        );
        assert_eq!(
            plan.lig_sites,
            vec![LigSite {
                components: vec![10],
                out: 55
            }]
        );
        assert_eq!(plan.units.len(), 1);
        assert_eq!(plan.units[0].glyph, 55);
        // colored copy of the *shaped* glyph
        assert_eq!(plan.alt_glyphs.len(), 1);
        assert_eq!(plan.alt_glyphs[0].0, 55);
    }

    #[test]
    fn empty_shaping_degrades_block_to_classic() {
        // the shaper returns nothing (misalignment upstream): the block
        // falls back to the per-character view and still colors
        let plan = plan_for_themes(&[("ab", &[(0, 2, Some(BLUE))])], &[], &[], Some(&[]));
        assert!(plan.lig_rules.is_empty());
        assert_eq!(plan.rules.len(), 1);
        assert_eq!(plan.rules[0].input.len(), 2);
        assert!(plan.warnings.iter().any(|w| w.contains("aligned")));
    }

    #[test]
    fn adjacent_sites_backtrack_sees_restored_glyphs() {
        // "== =" shapes to two ligatures (90 for the first pair, 91 for the
        // last pair with the repeated char): the second rule's backtrack
        // must reference glyph 90 in restored space, not the members
        let plan = plan_for_themes(&[("abab", &[])], &[], &[], Some(&[sg(90, 0), sg(91, 2)]));
        assert_eq!(plan.lig_rules.len(), 2, "{:?}", plan.lig_rules);
        let second = plan
            .lig_rules
            .iter()
            .find(|r| plan.lig_sites[r.site as usize].out == 91)
            .expect("rule for the second site");
        assert_eq!(second.backtrack.len(), 1, "{second:?}");
        let backtrack_class = second.backtrack[0];
        let out90_class = plan.class_of[&90];
        assert_eq!(backtrack_class, out90_class, "backtrack references 90");
    }

    #[test]
    fn matcher_finds_exact_and_overlapping() {
        // two "abab"/"ab" texts flattened; the matcher is a pure value
        // index — block semantics are enforced by the separators in seq
        let seq = vec![1, 2, 1, 2, 1, 2];
        let m = Matcher::new(seq);
        let mut found = m.matches(0, 2).to_vec(); // "ab"
        found.sort_unstable();
        assert_eq!(found, vec![0, 2, 4]);
        let mut found = m.matches(1, 2).to_vec(); // "ba"
        found.sort_unstable();
        assert_eq!(found, vec![1, 3]);
        let mut found = m.matches(0, 4).to_vec(); // "abab"
        found.sort_unstable();
        assert_eq!(found, vec![0, 2]);
        assert_eq!(m.matches(0, 6).to_vec(), vec![0]); // whole sequence
    }

    #[test]
    fn matcher_all_equal_values_terminates() {
        // degenerate input: every value equal; one suffix is a prefix of the
        // next, which must still terminate and match everywhere
        let m = Matcher::new(vec![-1, -1, -1]);
        let mut found = m.matches(1, 1).to_vec();
        found.sort_unstable();
        assert_eq!(found, vec![0, 1, 2]);
    }

    #[test]
    fn matcher_matches_brute_force() {
        // pseudo-random values; the suffix array must agree with a naive
        // scan for every (start, len) query window
        let mut x = 12345u64;
        let mut seq = Vec::new();
        for i in 0..500 {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            seq.push(((x >> 33) % 7) as i32);
            if i % 13 == 0 {
                seq.push(-1);
            }
        }
        let m = Matcher::new(seq.clone());
        let n = seq.len();
        for start in 0..n {
            for len in 1..=(n - start).min(6) {
                let pat = seq[start..start + len].to_vec();
                let mut brute: Vec<u32> = (0..n)
                    .filter(|&p| p + len <= n && seq[p..p + len] == pat)
                    .map(|p| p as u32)
                    .collect();
                brute.sort_unstable();
                let mut sa = m.matches(start, len).to_vec();
                sa.sort_unstable();
                assert_eq!(sa, brute, "pattern at {start} len {len}");
            }
        }
    }
}
