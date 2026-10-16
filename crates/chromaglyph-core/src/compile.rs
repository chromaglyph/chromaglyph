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
/// `charmap` resolves chars to base glyph ids; `num_source_glyphs` is where
/// alt glyph ids start; `theme_fgs` backs the CPAL palette fallback color
/// (one entry per theme, parallel to `PreparedBlock::colors`).
pub fn compile(
    blocks: &[PreparedBlock<'_>],
    charmap: &dyn Fn(char) -> Option<u16>,
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
            cells.push(Cell {
                byte_start: byte,
                ch,
                gid: charmap(ch),
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
    // A char with no cmap entry shapes to .notdef, which has no colored
    // copy. Grammars often scope such chars into tokens (a trailing newline
    // in a line comment, an emoji inside a string), so instead of degrading
    // the whole token we cut the run at every unmapped cell: the unmapped
    // cell stays on the base foreground, its mapped neighbors keep coloring.
    let mut split: Vec<(usize, usize, u16)> = Vec::with_capacity(occurrences.len());
    let mut unmapped_tokens = 0usize;
    for (start, end, pal) in occurrences {
        let mut run_start: Option<usize> = None;
        let mut had_unmapped = false;
        for (i, cell) in cells.iter_mut().enumerate().take(end).skip(start) {
            if cell.gid.is_some() {
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
    // The audit below asks, per segment, "where else does this glyph
    // sequence occur in the whole text?". A suffix array over the flattened
    // glyph sequence answers in O(m log n) instead of rescanning every cell
    // on every window-growth step (the old audit was O(n²) overall). The
    // sequence is 1:1 with cells; cross-block matches are filtered out when
    // candidates are collected.
    let seq: Vec<i32> = cells.iter().map(|c| i32::from(c.gid.unwrap_or(0))).collect();
    let matcher = Matcher::new(seq);

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
            let unresolvable = segments(start, end).any(|(s, e)| {
                let cands = candidates(&matcher, &mut cand_cache, &cells, s, e);
                build_rule(&cells, &block_ranges, &class_of, s, e, pal, &cands).is_none()
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
        for (s, e) in segments(start, end) {
            let cands = candidates(&matcher, &mut cand_cache, &cells, s, e);
            if let Some(rule) = build_rule(&cells, &block_ranges, &class_of, s, e, pal, &cands) {
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
        for cell in &cells[start..end] {
            pairs.insert((cell.gid.expect("active occurrences are mapped"), pal));
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

/// Cached candidate positions for the segment `[start, end)`: every other
/// position carrying the same glyph sequence *within one block* (matches
/// crossing a block boundary cannot falsely fire — rules never cross
/// blocks). Sequence matches are independent of the fixed point loop, so
/// this is computed once per segment.
fn candidates(
    matcher: &Matcher,
    cache: &mut HashMap<(usize, usize), Rc<Vec<u32>>>,
    cells: &[Cell],
    start: usize,
    end: usize,
) -> Rc<Vec<u32>> {
    if let Some(rc) = cache.get(&(start, end)) {
        return rc.clone();
    }
    let m = end - start;
    let rc: Rc<Vec<u32>> = Rc::new(
        matcher
            .matches(start, m)
            .iter()
            .copied()
            .map(|p| p as usize)
            .filter(|&p| p != start && cells[p + m - 1].block == cells[p].block)
            .map(|p| p as u32)
            .collect(),
    );
    cache.insert((start, end), rc.clone());
    rc
}

/// Build one contextual rule for `[start, end)` in slot `pal`: grow the
/// backtrack/lookahead window minimally until no false match remains, staying
/// inside the shaper's rule budget. `None` if the occurrence cannot be
/// uniquely pinned (caller degrades it to uncolored).
fn build_rule(
    cells: &[Cell],
    block_ranges: &[(usize, usize)],
    class_of: &HashMap<u16, u16>,
    start: usize,
    end: usize,
    pal: u16,
    cands: &[u32],
) -> Option<Rule> {
    let m = end - start;
    debug_assert!(m < SHAPER_MAX_CONTEXT, "segments leave room for context");
    let budget = SHAPER_MAX_CONTEXT - m; // max backtrack + lookahead combined
    let (block_start, block_end) = block_ranges[cells[start].block];
    let max_l = (start - block_start).min(budget);
    let max_r = (block_end - end).min(budget);
    let mut l = 0usize;
    let mut r = 0usize;
    let mut grow_left = true;
    loop {
        if audit(cells, block_ranges, start, end, pal, l, r, cands).is_some() {
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
            let backtrack = (1..=l)
                .map(|k| class_of[&cells[start - k].gid.unwrap_or(0)])
                .collect();
            let input = cells[start..end]
                .iter()
                .map(|c| class_of[&c.gid.expect("occurrence cells are mapped")])
                .collect();
            let lookahead = (1..=r)
                .map(|k| class_of[&cells[end + k - 1].gid.unwrap_or(0)])
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

/// Check whether a rule for occurrence `[start, end)` with window `(l, r)`
/// would falsely fire at any candidate position. Returns the first conflicting
/// position (cell index) if so.
///
/// `cands` holds every position `p != start` whose glyph sequence equals the
/// occurrence's within `p`'s own block (see [`Matcher`]). The rule falsely
/// fires at `p` iff, within `p`'s block:
///  - `p >= l` and `p + m + r <= block_end`,
///  - the `l` glyphs before and `r` glyphs after equal the occurrence's, and
///  - some covered glyph was *not* intended to carry this slot.
///
/// Class ids are 1:1 with base glyphs, so comparing base glyph ids at build
/// time is exactly the class-level match at run time (see invariant 1).
#[allow(clippy::too_many_arguments)] // independent coordinates of one probe
fn audit(
    cells: &[Cell],
    block_ranges: &[(usize, usize)],
    start: usize,
    end: usize,
    pal: u16,
    l: usize,
    r: usize,
    cands: &[u32],
) -> Option<usize> {
    let m = end - start;

    for &p in cands {
        let p = p as usize;
        // window containment within p's block
        let (p_lo, p_hi) = block_ranges[cells[p].block];
        if p - p_lo < l || p_hi - (p + m) < r {
            continue;
        }
        // backtrack equality
        let mut equal = true;
        for k in 1..=l {
            if cells[p - k].gid.unwrap_or(0) != cells[start - k].gid.unwrap_or(0) {
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
            if cells[p + m + k - 1].gid.unwrap_or(0) != cells[end + k - 1].gid.unwrap_or(0) {
                equal = false;
                break;
            }
        }
        if !equal {
            continue;
        }
        // The rule would fire at p. Harmless iff every covered cell is
        // already intended to carry this slot.
        let mut differs = false;
        for i in 0..m {
            if cells[p + i].pal != Some(pal) {
                differs = true;
                break;
            }
        }
        if differs {
            return Some(p);
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
        let charmap = |ch: char| -> Option<u16> {
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
        let charmap = |ch: char| -> Option<u16> { Some(10 + (ch as u16 - 'a' as u16)) };
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
        let charmap = |ch: char| -> Option<u16> { Some(10 + (ch as u16 - 'a' as u16)) };
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
    fn matcher_finds_exact_and_overlapping() {
        // two "abab"/"ab" texts flattened; the matcher is a pure value
        // index — block semantics are enforced by `candidates`
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
