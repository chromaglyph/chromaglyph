//! Stage 3 of the pipeline: token occurrences → chain-context GSUB rules.
//!
//! This is the heart of chromaglyph's "exact mode". Because the content is
//! known at build time, every colored token occurrence becomes one contextual
//! substitution rule whose backtrack/lookahead is the *minimal* window that
//! uniquely pins the occurrence in the whole (multi-block) text. There is no
//! lexer state machine to carry through the font, unlike the original
//! FontWithASyntaxHighlighter design.
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

use std::collections::{HashMap, HashSet};

use crate::error::BuildError;
use crate::spec::{Color, Theme, Token};

/// Glyph-level view of one char cell in the flattened text.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// Byte offset of this char in its block (used as the shaping cluster).
    pub byte_start: usize,
    pub ch: char,
    /// Base glyph id from cmap; `None` for unmapped chars.
    pub gid: Option<u16>,
    /// Index of the owning block.
    pub block: usize,
    /// Intended palette index after substitution; `None` = keep base glyph.
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
    /// Palette index substituted onto every input position.
    pub pal: u16,
}

/// Everything the font builder and the verifier need to know about the plan:
/// colored glyph copies, the class definition, and the final rule set.
#[derive(Debug)]
pub struct GlyphPlan {
    pub cells: Vec<Cell>,
    /// (base_gid, pal_idx, alt_gid) sorted by (base_gid, pal_idx);
    /// alt gids continue after the source font's glyph ids.
    pub alt_glyphs: Vec<(u16, u16, u16)>,
    /// glyph id -> class id (base glyphs and their alt copies share a class).
    pub class_of: HashMap<u16, u16>,
    pub rules: Vec<Rule>,
    /// Deduplicated palette colors, index = CPAL palette index.
    pub palette: Vec<Color>,
    /// Glyph count of the source font (alt glyph ids start here).
    pub num_source_glyphs: u16,
    /// Per-block byte ranges into the concatenated cell vector.
    pub block_ranges: Vec<(usize, usize)>,
    /// Number of tokens that ended up uncolored (degraded or theme choice).
    pub uncolored_tokens: usize,
    pub total_tokens: usize,
    /// Non-fatal degradations (e.g. tokens containing chars with no glyph).
    pub warnings: Vec<String>,
}

/// A block whose text has already been tokenized.
pub struct PreparedBlock<'a> {
    pub id: String,
    pub text: &'a str,
    pub tokens: Vec<Token<'a>>,
}

/// HarfBuzz/rustybuzz never apply a contextual rule whose total window
/// (backtrack + input + lookahead) exceeds this many glyphs.
const SHAPER_MAX_CONTEXT: usize = 64;
/// Long occurrences (comments, strings) are colored by several chained
/// segment rules of at most this many input glyphs, leaving room for context.
const SEGMENT_MAX_INPUT: usize = 48;

/// Split `[start, end)` into roughly balanced segments.
fn segments(start: usize, end: usize) -> impl Iterator<Item = (usize, usize)> {
    let m = end - start;
    let count = m.div_ceil(SEGMENT_MAX_INPUT).max(1);
    let seg = m.div_ceil(count);
    (0..m)
        .step_by(seg)
        .map(move |off| (start + off, (start + off + seg).min(end)))
}

/// Build one contextual rule for `[start, end)` in palette `pal`: grow the
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
        if audit(cells, block_ranges, start, end, pal, l, r).is_some() {
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

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max_chars).collect();
        format!("{cut}…")
    }
}

/// Compile tokenized blocks + theme into a glyph plan.
///
/// `charmap` resolves chars to base glyph ids; `num_source_glyphs` is where
/// alt glyph ids start.
pub fn compile(
    blocks: &[PreparedBlock<'_>],
    theme: &Theme,
    charmap: &dyn Fn(char) -> Option<u16>,
    num_source_glyphs: u16,
) -> Result<GlyphPlan, BuildError> {
    // ---- palette: dedupe theme colors ------------------------------------
    let mut palette: Vec<Color> = Vec::new();
    let mut color_index: HashMap<Color, u16> = HashMap::new();
    let mut scope_pal: HashMap<&str, Option<u16>> = HashMap::new();
    for token_scope in blocks.iter().flat_map(|b| b.tokens.iter().map(|t| t.scope)) {
        if scope_pal.contains_key(token_scope) {
            continue;
        }
        match theme.color_for(token_scope) {
            Some(Some(color)) => {
                let idx = match color_index.get(&color) {
                    Some(&idx) => idx,
                    None => {
                        let idx = palette.len() as u16;
                        palette.push(color);
                        color_index.insert(color, idx);
                        idx
                    }
                };
                scope_pal.insert(token_scope, Some(idx));
            }
            _ => {
                scope_pal.insert(token_scope, None);
            }
        }
    }

    // ---- flatten blocks into cells ---------------------------------------
    let mut cells: Vec<Cell> = Vec::new();
    let mut block_ranges: Vec<(usize, usize)> = Vec::new();
    let mut total_tokens = 0usize;
    let mut uncolored_tokens = 0usize;
    for (block_idx, block) in blocks.iter().enumerate() {
        let start = cells.len();
        let text = block.text;
        let mut token_iter = block.tokens.iter().peekable();
        let mut next_token = token_iter.next();
        for (byte, ch) in text.char_indices() {
            // advance to the token covering `byte`
            while let Some(tok) = next_token {
                if byte >= tok.end {
                    next_token = token_iter.next();
                } else {
                    break;
                }
            }
            let scope = next_token
                .filter(|tok| byte >= tok.start)
                .map(|tok| tok.scope)
                .unwrap_or("unknown");
            let pal = scope_pal.get(scope).copied().flatten();
            cells.push(Cell {
                byte_start: byte,
                ch,
                gid: charmap(ch),
                block: block_idx,
                pal,
            });
        }
        let end = cells.len();
        // sanity: token coverage must be exact
        if let Some(last) = block.tokens.last() {
            if last.end != text.len() {
                return Err(BuildError::BadTokens(format!(
                    "block '{}': tokens end at byte {}, text is {} bytes",
                    block.id,
                    last.end,
                    text.len()
                )));
            }
        }
        total_tokens += block.tokens.len();
        block_ranges.push((start, end));
    }

    // ---- occurrences: maximal runs of cells with the same palette --------
    // A token never spans two palettes, and adjacent same-palette tokens are
    // merged here; rules operate on glyph runs, not on token boundaries.
    let mut occurrences: Vec<(usize, usize, u16)> = Vec::new(); // (start, end, pal) cell idx
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
    // count uncolored tokens for the report (tokens whose scope has no color)
    for block in blocks {
        for tok in &block.tokens {
            if scope_pal.get(tok.scope).copied().flatten().is_none() {
                uncolored_tokens += 1;
            }
        }
    }

    // ---- degrade occurrences that contain unmapped chars ------------------
    // A char with no cmap entry shapes to .notdef, which has no colored copy;
    // such tokens stay on the base foreground color and a warning is recorded
    // instead of failing the build.
    let mut warnings: Vec<String> = Vec::new();
    let occurrences: Vec<(usize, usize, u16)> = occurrences
        .into_iter()
        .filter(|&(start, end, pal)| {
            if let Some(cell) = cells[start..end].iter().find(|c| c.gid.is_none()) {
                let text: String = cells[start..end].iter().map(|c| c.ch).collect();
                warnings.push(format!(
                    "block '{}': token {:?} contains {:?} which has no glyph; left uncolored",
                    blocks[cell.block].id, text, cell.ch
                ));
                for c in &mut cells[start..end] {
                    c.pal = None;
                }
                let _ = pal;
                false
            } else {
                true
            }
        })
        .collect();

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

    // ---- minimal distinguishing contexts + global audit -----------------
    // HarfBuzz/rustybuzz never apply a contextual rule whose total window
    // (backtrack + input + lookahead) exceeds 64 glyphs, and long tokens
    // (comments, strings) must therefore be colored by several chained
    // segment rules. Unresolvable occurrences degrade to uncolored + warning
    // instead of failing the build.
    let mut active: Vec<(usize, usize, u16)> = occurrences;
    loop {
        let failures: Vec<usize> = active
            .iter()
            .enumerate()
            .filter(|&(_, &(start, end, pal))| {
                segments(start, end)
                    .any(|(s, e)| build_rule(&cells, &block_ranges, &class_of, s, e, pal).is_none())
            })
            .map(|(i, _)| i)
            .collect();
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
            if let Some(rule) = build_rule(&cells, &block_ranges, &class_of, s, e, pal) {
                rules.push(rule);
            }
        }
    }

    // ---- alt glyphs: one colored copy per (base glyph, palette) ----------
    // Computed after degradation so degraded occurrences contribute nothing.
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

    Ok(GlyphPlan {
        cells,
        alt_glyphs,
        class_of,
        rules,
        palette,
        num_source_glyphs,
        block_ranges,
        uncolored_tokens,
        total_tokens,
        warnings,
    })
}

/// Check whether a rule for occurrence `[start, end)` with window `(l, r)`
/// would falsely fire at any other position. Returns the first conflicting
/// position (cell index) if so.
///
/// The rule matches at position `p` iff, within `p`'s own block:
///  - `p >= l` and `p + m + r <= block_end`,
///  - the glyph sequence at `p` equals the occurrence's, and
///  - the `l` glyphs before and `r` glyphs after equal the occurrence's.
///
/// Class ids are 1:1 with base glyphs, so comparing base glyph ids at build
/// time is exactly the class-level match at run time (see invariant 1).
fn audit(
    cells: &[Cell],
    block_ranges: &[(usize, usize)],
    start: usize,
    end: usize,
    pal: u16,
    l: usize,
    r: usize,
) -> Option<usize> {
    let m = end - start;

    let first_gid = cells[start].gid.unwrap_or(0);
    for p in 0..cells.len() {
        if p == start {
            continue;
        }
        if p + m > cells.len() {
            break;
        }
        if cells[p + m - 1].block != cells[p].block {
            continue;
        }
        if cells[p].gid.unwrap_or(0) != first_gid {
            continue;
        }
        // glyph sequence equality
        let mut equal = true;
        for i in 0..m {
            if cells[p + i].gid.unwrap_or(0) != cells[start + i].gid.unwrap_or(0) {
                equal = false;
                break;
            }
        }
        if !equal {
            continue;
        }
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
        // already intended to carry this palette.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Block, Theme};

    type TokSpec = (usize, usize, &'static str);
    type BlockSpec<'a> = (&'a str, &'a str, &'a [TokSpec]);

    fn plan_for(texts: &[BlockSpec<'_>]) -> GlyphPlan {
        // toy charmap: ASCII 'a'..='z' -> gid 10.., ' ' -> 2, everything maps
        let charmap = |ch: char| -> Option<u16> {
            match ch {
                'a'..='z' => Some(10 + (ch as u16 - 'a' as u16)),
                ' ' => Some(2),
                _ => Some(3),
            }
        };
        let mut theme = Theme::builtin_dark();
        theme
            .scopes
            .insert("keyword".into(), Some(Color::rgb(1, 1, 1)));
        theme
            .scopes
            .insert("string".into(), Some(Color::rgb(2, 2, 2)));
        theme
            .scopes
            .insert("identifier".into(), Some(Color::rgb(3, 3, 3)));
        let blocks: Vec<PreparedBlock> = texts
            .iter()
            .map(|(id, text, toks)| PreparedBlock {
                id: id.to_string(),
                text,
                tokens: toks
                    .iter()
                    .map(|(s, e, sc)| Token {
                        start: *s,
                        end: *e,
                        scope: sc,
                    })
                    .collect(),
            })
            .collect();
        // fill remaining gaps with "punctuation" tokens
        let blocks = fill_gaps(blocks);
        compile(&blocks, &theme, &charmap, 100).unwrap()
    }

    fn fill_gaps(blocks: Vec<PreparedBlock>) -> Vec<PreparedBlock> {
        blocks
            .into_iter()
            .map(|mut b| {
                let mut filled: Vec<Token> = Vec::new();
                let mut cursor = 0;
                let mut sorted = b.tokens.clone();
                sorted.sort_by_key(|t| t.start);
                for tok in sorted {
                    if tok.start > cursor {
                        filled.push(Token {
                            start: cursor,
                            end: tok.start,
                            scope: "punctuation",
                        });
                    }
                    filled.push(tok);
                    cursor = tok.end;
                }
                if cursor < b.text.len() {
                    filled.push(Token {
                        start: cursor,
                        end: b.text.len(),
                        scope: "punctuation",
                    });
                }
                b.tokens = filled;
                b
            })
            .collect()
    }

    fn tokens_of(text: &str) -> Vec<(usize, usize, &str)> {
        crate::tokenize::tokenize(text, "rust")
            .iter()
            .map(|t| (t.start, t.end, t.scope))
            .collect()
    }

    #[test]
    fn minimal_context_same_color_shares_rule() {
        // "let" twice, same color -> after dedup a single rule with empty context
        let text = "let a; let b;";
        let toks = tokens_of(text);
        let plan = plan_for(&[("b1", text, &toks)]);
        let let_rules: Vec<&Rule> = plan.rules.iter().filter(|r| r.input.len() == 3).collect();
        assert!(!let_rules.is_empty());
    }

    #[test]
    fn same_text_different_color_gets_context() {
        // "ab" as keyword (blue) and "ab" as identifier (red) separated by one
        // filler char; contexts must disambiguate them.
        let text = "ab ab";
        let toks: Vec<(usize, usize, &str)> = vec![
            (0, 2, "keyword"),
            (2, 3, "punctuation"),
            (3, 5, "identifier"),
        ];
        let plan = plan_for(&[("b1", text, &toks)]);
        assert_eq!(plan.rules.len(), 2);
        assert!(
            plan.rules
                .iter()
                .any(|r| !r.backtrack.is_empty() || !r.lookahead.is_empty()),
            "some rule must carry context: {:#?}",
            plan.rules
        );
        // the two rules must not both fire at the same position: verify by
        // checking their windows differ
        let r0 = &plan.rules[0];
        let r1 = &plan.rules[1];
        assert_ne!(
            (r0.backtrack.clone(), r0.lookahead.clone()),
            (r1.backtrack.clone(), r1.lookahead.clone())
        );
    }

    #[test]
    fn prefix_protection() {
        // "for" keyword + "format" identifier: the for rule must not fire
        // inside format. After audit, for's rule carries lookahead.
        let text = "for format";
        let toks = tokens_of(text);
        let plan = plan_for(&[("b1", text, &toks)]);
        let for_rules: Vec<&Rule> = plan.rules.iter().filter(|r| r.input.len() == 3).collect();
        assert!(!for_rules.is_empty());
        assert!(
            for_rules
                .iter()
                .all(|r| !r.lookahead.is_empty() || !r.backtrack.is_empty()),
            "for rule needs context to avoid firing inside 'format': {:#?}",
            for_rules
        );
    }

    #[test]
    fn periodic_text_resolves() {
        // "ab ab ab": occurrences at 0, 3, 6 (cell indices); all same color is
        // fine; mixed colors must still resolve via clamped windows.
        let text = "ababab";
        let toks: Vec<(usize, usize, &str)> =
            vec![(0, 2, "keyword"), (2, 4, "identifier"), (4, 6, "string")];
        let plan = plan_for(&[("b1", text, &toks)]);
        assert_eq!(plan.rules.len(), 3);
    }

    #[test]
    fn cross_block_identical_text_degrades_instead_of_failing() {
        // "ab" as keyword in block 1 and as identifier in block 2 with both
        // blocks containing *only* "ab": any rule would fire in both blocks,
        // so both occurrences must degrade to uncolored (never fail).
        let t1 = "ab";
        let t2 = "ab";
        let toks1: Vec<TokSpec> = vec![(0, 2, "keyword")];
        let toks2: Vec<TokSpec> = vec![(0, 2, "identifier")];
        let plan = plan_for(&[("b1", t1, &toks1), ("b2", t2, &toks2)]);
        assert!(
            !plan.warnings.is_empty(),
            "expected degradation warnings, plan: {plan:?}"
        );
        assert!(plan.cells.iter().all(|c| c.pal.is_none()));
        assert!(
            plan.alt_glyphs.is_empty(),
            "no colored copies for degraded cells"
        );
    }

    #[test]
    fn blocks_are_independent_shaping_runs() {
        // Same text, same color in two blocks: fine, and windows stay empty
        // (block boundary alone separates nothing because color matches).
        let t = "let x;";
        let toks = tokens_of(t);
        let plan = plan_for(&[("b1", t, &toks), ("b2", t, &toks)]);
        assert!(!plan.rules.is_empty());
    }

    #[test]
    fn alt_glyphs_deduped() {
        // 'l','e','t' colored keyword in both blocks -> 3 pairs only
        let t = "let";
        let toks: Vec<(usize, usize, &str)> = vec![(0, 3, "keyword")];
        let plan = plan_for(&[("b1", t, &toks), ("b2", t, &toks)]);
        assert_eq!(plan.alt_glyphs.len(), 3);
        assert_eq!(plan.palette.len(), 1);
    }

    #[test]
    fn block_api_compiles() {
        let _ = Block {
            id: "x".into(),
            lang: "rust".into(),
            text: "let",
        };
    }
}
