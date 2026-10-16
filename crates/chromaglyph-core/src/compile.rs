//! Stage 3 of the pipeline: per-character colors → chain-context GSUB rules.
//!
//! This is the heart of chromaglyph's "exact mode". Because the content is
//! known at build time, every colored token occurrence becomes one contextual
//! substitution rule whose backtrack/lookahead is the *minimal* window that
//! uniquely pins the occurrence in the whole (multi-block) text. There is no
//! lexer state machine to carry through the font, unlike the original
//! FontWithASyntaxHighlighter design.
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
pub fn compile(
    blocks: &[PreparedBlock<'_>],
    charmap: &dyn Fn(char, Option<char>) -> Option<u16>,
    num_source_glyphs: u16,
    theme_fgs: &[Color],
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
    let uncolored_cells = cells.iter().filter(|c| c.pal.is_none()).count();

    // ---- shaping view -----------------------------------------------------
    // How the shaper of *this* font turns cells into glyphs: hidden
    // controls (variation selectors, zero-width format controls) produce no
    // glyph at all; unmapped chars produce .notdef and, in a browser, leave
    // this font's shaping run entirely (font fallback itemization), so they
    // are hard separators for rule contexts. `seq` models exactly that
    // adjacency: one entry per mapped cell, `-1` at every unmapped cell and
    // block boundary. Rules and their contexts live in this space; `-1`
    // never enters a rule because windows stop at separators.
    // `cell_at` is aligned with `seq` positions (u32::MAX at separators,
    // never dereferenced — matches cover only real glyph positions).
    let mut shaped_pos: Vec<i32> = vec![-1; cells.len()];
    let mut cell_at: Vec<u32> = Vec::new();
    let mut seq: Vec<i32> = Vec::new();
    for &(lo, hi) in &block_ranges {
        for i in lo..hi {
            if is_shaper_hidden(cells[i].ch) {
                continue;
            }
            match cells[i].gid {
                Some(gid) => {
                    shaped_pos[i] = seq.len() as i32;
                    cell_at.push(i as u32);
                    seq.push(i32::from(gid));
                }
                None => {
                    cell_at.push(u32::MAX);
                    seq.push(-1);
                }
            }
        }
        cell_at.push(u32::MAX);
        seq.push(-1); // matches never cross block boundaries
    }

    // ---- occurrences: maximal runs of cells with the same slot -----------
    // Adjacent same-slot tokens merge; rules operate on glyph runs.
    let mut occurrences: Vec<(usize, usize, u16)> = Vec::new();
    {
        let mut i = 0;
        while i < cells.len() {
            if let Some(pal) = cells[i].pal {
                let start = i;
                let block = cells[i].block;
                while i < cells.len() && cells[i].pal == Some(pal) && cells[i].block == block {
                    i += 1;
                }
                occurrences.push((start, i, pal));
            } else {
                i += 1;
            }
        }
    }

    // ---- split occurrences at unmapped chars ------------------------------
    // A char with no cmap entry and no shaper-side hiding is a barrier: it
    // shapes to .notdef here, and in a browser it moves to a fallback font,
    // breaking the shaping run. Grammars often scope such chars into tokens
    // (a trailing newline in a line comment, an emoji inside a string), so
    // instead of degrading the whole token we cut the run at every barrier:
    // the barrier cell stays on the base foreground, its mapped neighbors
    // keep coloring. Hidden controls (variation selectors) are *not*
    // barriers — the shaper drops them, so the mapped text around them is
    // contiguous at shaping time and the run continues through them.
    let mut split: Vec<(usize, usize, u16)> = Vec::with_capacity(occurrences.len());
    let mut unmapped_tokens = 0usize;
    for (start, end, pal) in occurrences {
        let mut run_start: Option<usize> = None;
        let mut had_unmapped = false;
        for (i, cell) in cells.iter_mut().enumerate().take(end).skip(start) {
            if cell.gid.is_some() || is_shaper_hidden(cell.ch) {
                run_start.get_or_insert(i);
            } else {
                if let Some(s) = run_start.take() {
                    split.push((s, i, pal));
                }
                cell.pal = None;
                had_unmapped = true;
            }
        }
        if let Some(s) = run_start {
            split.push((s, end, pal));
        }
        if had_unmapped {
            unmapped_tokens += 1;
        }
    }
    if unmapped_tokens > 0 {
        warnings.push(format!(
            "{unmapped_tokens} token(s) contain chars with no glyph in the font \
             (typically trailing newlines); those chars stay uncolored, coloring \
             continues around them"
        ));
    }
    let occurrences = split;

    // ---- classes: one class per distinct base glyph ----------------------
    // Unmapped chars shape to .notdef (gid 0), so gid 0 gets a class too and
    // rule contexts may legally cross unmapped positions.
    let mut base_gids: Vec<u16> = cells.iter().map(|c| c.gid.unwrap_or(0)).collect();
    base_gids.sort_unstable();
    base_gids.dedup();
    let mut class_of: HashMap<u16, u16> = HashMap::with_capacity(base_gids.len() * 2);
    for (class, gid) in base_gids.iter().enumerate() {
        class_of.insert(*gid, (class + 1) as u16);
    }

    // ---- global match index ----------------------------------------------
    // A suffix array over `seq` answers "where else does this glyph
    // sequence occur?" in O(m log n) instead of rescanning every cell on
    // every window-growth step.
    let matcher = Matcher::new(seq.clone());

    // ---- minimal distinguishing contexts + global audit -----------------
    // Unresolvable occurrences degrade to uncolored + warning instead of
    // failing the build; degrading can resolve other occurrences' conflicts,
    // so the loop runs to a fixed point. Sequence matches never change
    // during the loop (only intended slots do), so each segment's candidate
    // positions are computed once and reused across iterations.
    let mut cand_cache: HashMap<(usize, usize), Rc<Vec<u32>>> = HashMap::new();
    let mut active: Vec<(usize, usize, u16)> = occurrences;
    loop {
        let mut failures: Vec<usize> = Vec::new();
        for (i, &(start, end, pal)) in active.iter().enumerate() {
            let Some((s, e)) = shaped_span(&shaped_pos, start, end) else {
                continue; // no glyph to color (only hidden controls)
            };
            let unresolvable = segments(s, e).any(|(ss, se)| {
                let cands = candidates(&matcher, &mut cand_cache, ss, se);
                build_rule(&seq, &cells, &cell_at, &class_of, ss, se, pal, &cands).is_none()
            });
            if unresolvable {
                failures.push(i);
            }
        }
        if failures.is_empty() {
            break;
        }
        for &i in &failures {
            let (start, end, _pal) = active[i];
            let text: String = cells[start..end].iter().map(|c| c.ch).collect();
            warnings.push(format!(
                "block '{}': occurrence of {:?} ({} cells) cannot be uniquely \
                 distinguished within the 64-glyph shaper context; left uncolored",
                blocks[cells[start].block].id,
                truncate(&text, 40),
                end - start
            ));
            for cell in &mut cells[start..end] {
                cell.pal = None;
            }
        }
        for &i in failures.iter().rev() {
            active.swap_remove(i);
        }
    }
    let mut rules: Vec<Rule> = Vec::new();
    for &(start, end, pal) in &active {
        let Some((s, e)) = shaped_span(&shaped_pos, start, end) else {
            continue;
        };
        for (ss, se) in segments(s, e) {
            let cands = candidates(&matcher, &mut cand_cache, ss, se);
            if let Some(rule) = build_rule(&seq, &cells, &cell_at, &class_of, ss, se, pal, &cands)
            {
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

    // ---- alt glyphs: one colored copy per (base glyph, slot) -------------
    let mut pairs: HashSet<(u16, u16)> = HashSet::new();
    for &(start, end, pal) in &active {
        if let Some((s, e)) = shaped_span(&shaped_pos, start, end) {
            for &g in &seq[s..e] {
                pairs.insert((g as u16, pal));
            }
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

    Ok(GlyphPlan {
        cells,
        alt_glyphs,
        class_of,
        rules,
        slots,
        theme_fgs: theme_fgs.to_vec(),
        num_source_glyphs,
        plain_glyphs: Vec::new(),
        block_ranges,
        uncolored_cells,
        warnings,
    })
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

/// Shaped span `[s, e)` of a cell-range occurrence: the shaped positions
/// of its mapped cells (contiguous in `seq`: hidden controls contribute
/// nothing and occurrences contain no barriers after splitting). `None`
/// when the occurrence has no mapped cell at all.
fn shaped_span(shaped_pos: &[i32], start: usize, end: usize) -> Option<(usize, usize)> {
    let mut s = None;
    let mut e = 0;
    for &i in &shaped_pos[start..end] {
        if i >= 0 {
            let p = i as usize;
            s.get_or_insert(p);
            e = p + 1;
        }
    }
    s.map(|s| (s, e))
}

/// Chars the shaper hides entirely (produces no glyph for) when the font
/// does not map them: variation selectors plus zero-width/bidi format
/// controls. These are transparent to rule building: they never split an
/// occurrence and occupy no position in the shaping view.
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

/// Cached candidate positions for the segment `[s, e)` in shaping space:
/// every other shaped position carrying the same glyph run (separators in
/// `seq` keep matches inside one block and one barrier-free run). Sequence
/// matches are independent of the fixed point loop, so this is computed
/// once per segment.
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

/// Build one contextual rule for the shaped segment `[s, e)` in slot
/// `pal`: grow the backtrack/lookahead window minimally until no false
/// match remains, staying inside the shaper's rule budget. `None` if the
/// occurrence cannot be uniquely pinned (caller degrades it to uncolored).
#[allow(clippy::too_many_arguments)] // independent coordinates of one probe
fn build_rule(
    seq: &[i32],
    cells: &[Cell],
    cell_at: &[u32],
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
    let max_l = (0..s).rev().take_while(|&k| seq[k] >= 0).count().min(budget);
    let max_r = (e..seq.len())
        .take_while(|&k| seq[k] >= 0)
        .count()
        .min(budget);
    let mut l = 0usize;
    let mut r = 0usize;
    let mut grow_left = true;
    loop {
        if audit(seq, cells, cell_at, s, e, pal, l, r, cands).is_some() {
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
            let backtrack = (1..=l).map(|k| class_of[&(seq[s - k] as u16)]).collect();
            let input = seq[s..e].iter().map(|&g| class_of[&(g as u16)]).collect();
            let lookahead = (1..=r)
                .map(|k| class_of[&(seq[e + k - 1] as u16)])
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

/// Check whether a rule for the shaped segment `[s, e)` with window `(l, r)`
/// would falsely fire at any candidate position. Returns the first
/// conflicting position (shaped index) if so.
///
/// A candidate `q` falsely fires iff its glyph run equals the occurrence's
/// and the `l` glyphs before and `r` glyphs after also match. Comparisons
/// happen in `seq` space, so a window crossing a separator (`-1`) can never
/// equal a window of real glyph ids — barrier and block clamping fall out
/// of the equality itself. A firing is harmless iff every covered cell
/// already intends this slot.
///
/// Class ids are 1:1 with base glyphs, so comparing glyph ids at build time
/// is exactly the class-level match at run time (see invariant 1).
#[allow(clippy::too_many_arguments)] // independent coordinates of one probe
fn audit(
    seq: &[i32],
    cells: &[Cell],
    cell_at: &[u32],
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
        if q < l || q + m + r > seq.len() {
            continue;
        }
        // backtrack equality (a `-1` separator never equals a glyph id)
        let mut equal = true;
        for k in 1..=l {
            if seq[q - k] != seq[s - k] {
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
            if seq[q + m + k - 1] != seq[e + k - 1] {
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
        let lo = cell_at[q] as usize;
        let hi = cell_at[q + m - 1] as usize + 1;
        if cells[lo..hi].iter().any(|c| c.pal != Some(pal)) {
            return Some(q);
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
            sa.sort_unstable_by(|&a, &b| {
                key(a as usize, &rank, k).cmp(&key(b as usize, &rank, k))
            });
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
        let hi = lo + self.sa[lo..].partition_point(|&s| {
            cmp_suffix(s as usize) == std::cmp::Ordering::Equal
        });
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
        plan_for_themes(texts, &[], &[])
    }

    /// Multi-theme variant: `themes` is extra per-theme color runs; theme 0
    /// uses `texts`' spans, theme k uses `themes[k]`' spans.
    fn plan_for_themes(
        texts: SpanBlocks<'_>,
        _extra: &[&[(usize, usize, Option<Color>)]],
        _extra_fgs: &[Color],
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
        compile(&blocks, &charmap, 100, &[Color::rgb(0xd4, 0xd4, 0xd4)]).unwrap()
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
        // "ab" blue then "ab" red: the two rules need distinguishing windows
        let plan = plan_for(&[("ab ab", &[(0, 2, Some(BLUE)), (3, 5, Some(RED))])]);
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
        // "for" blue + "format" red: the for rule must not fire inside format
        let plan = plan_for(&[("for format", &[(0, 3, Some(BLUE)), (4, 10, Some(RED))])]);
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
        let charmap = |ch: char, _vs: Option<char>| -> Option<u16> {
            Some(10 + (ch as u16 - 'a' as u16))
        };
        let mk = |color: Option<Color>| PreparedBlock {
            id: "b".into(),
            text: "ab",
            colors: vec![vec![color, color]],
        };
        let blocks = vec![mk(Some(BLUE)), mk(Some(RED))];
        let plan = compile(&blocks, &charmap, 100, &[Color::rgb(0xd4, 0xd4, 0xd4)]).unwrap();
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
        let charmap = |ch: char, _vs: Option<char>| -> Option<u16> {
            Some(10 + (ch as u16 - 'a' as u16))
        };
        let row = |color: Option<Color>, n: usize| vec![color; n];
        let blocks = vec![PreparedBlock {
            id: "b".into(),
            text: "ab",
            colors: vec![row(Some(BLUE), 2), row(Some(RED), 2)],
        }];
        let plan = compile(&blocks, &charmap, 100, &[BLUE, RED]).unwrap();
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
        let plan = compile(&blocks, &charmap, 100, &[Color::rgb(0xd4, 0xd4, 0xd4)]).unwrap();
        assert!(!plan.warnings.is_empty());
        assert!(
            plan.cells.iter().all(|c| c.pal.is_none() || c.gid.is_none()),
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
        let plan = compile(&blocks, &charmap, 100, &[Color::rgb(0xd4, 0xd4, 0xd4)]).unwrap();
        assert_eq!(plan.cells[0].gid, Some(42));
        assert_eq!(plan.alt_glyphs.len(), 1);
        assert_eq!(plan.alt_glyphs[0].0, 42, "colored copy of the UVS glyph");
        assert_eq!(plan.rules.len(), 1);
        assert_eq!(plan.rules[0].input.len(), 1);
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
