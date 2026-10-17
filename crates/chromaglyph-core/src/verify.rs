//! Stage 5 of the pipeline: the rustybuzz oracle.
//!
//! The produced font is shaped in-process with rustybuzz (the same shaping
//! algorithm HarfBuzz uses, which is what browsers run). For every block we
//! compare the shaped glyph stream against the plan's **units** — the glyph
//! the output font is supposed to produce per position (a plain cell's
//! glyph, a restored ligature glyph, or the colored copy of either).
//! Substitution is theme-independent — the same rules run under every
//! palette — so shaping is checked once, and then every CPAL palette is
//! read back and compared with the theme intent slot by slot.
//!
//! The standalone `verify_font` rebuilds the color intents from the input
//! text + themes alone (no source font at hand), so it cannot reconstruct
//! the build-time shaping that decided the ligatures. It therefore checks
//! the intents against the font's own COLR claims: every cluster span that
//! colors uniformly must render a colored copy of exactly that slot, and
//! every mixed/uncolored span must render an uncolored glyph. Which
//! *uncolored* glyph an uncolored ligature restores remains trusted from
//! the build (only `verify_plan` sees the true plan).

use std::collections::HashMap;

use read_fonts::{FontRef, TableProvider};
use rustybuzz::{Face, UnicodeBuffer};

use crate::compile::{GlyphPlan, PreparedBlock};
use crate::error::BuildError;
use crate::theme::ThemeSpec;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifyResult {
    pub ok: bool,
    pub glyphs_checked: usize,
    pub colored_glyphs: usize,
    pub palettes_checked: usize,
    pub failures: Vec<String>,
}

impl VerifyResult {
    pub fn ok() -> Self {
        VerifyResult {
            ok: true,
            glyphs_checked: 0,
            colored_glyphs: 0,
            palettes_checked: 0,
            failures: Vec::new(),
        }
    }

    pub(crate) fn fail(&mut self, msg: String) {
        self.ok = false;
        self.failures.push(msg);
    }
}

/// Shape every block with the produced font and diff against the plan.
pub fn verify_plan(ttf: &[u8], blocks: &[PreparedBlock<'_>], plan: &GlyphPlan) -> VerifyResult {
    let mut result = VerifyResult::ok();
    let mut alt_of: HashMap<(u16, u16), u16> = HashMap::new();
    for &(base, pal, alt) in &plan.alt_glyphs {
        alt_of.insert((base, pal), alt);
    }

    let face = match Face::from_slice(ttf, 0) {
        Some(face) => face,
        None => {
            let mut result = VerifyResult::ok();
            result.fail("rustybuzz cannot parse the produced font".into());
            return result;
        }
    };

    for (block_idx, &(lo, hi)) in plan.block_ranges.iter().enumerate() {
        let block = &blocks[block_idx];
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(block.text);
        let shaped = rustybuzz::shape(&face, &[], buffer);

        let infos = shaped.glyph_infos();
        // The shaper produces no glyph for hidden controls in browsers
        // (variation selectors unresolved by cmap14 get deleted), but
        // rustybuzz emits an extra glyph for them — alignment therefore
        // walks clusters, skipping glyphs whose cluster belongs to a
        // hidden cell. Every non-hidden cell expects exactly one glyph at
        // its own byte offset: the covered unit's glyph (colored copy or
        // base) for mapped cells, .notdef for unmapped ones.
        let mut unit_of_cell: HashMap<usize, &crate::compile::Unit> = HashMap::new();
        let mut member_cells: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for unit in plan.units.iter().filter(|u| u.block == block_idx) {
            unit_of_cell.insert(unit.cell as usize, unit);
            // a ligature's later member cells produce no glyph of their own
            for c in (unit.cell as usize + 1)..(unit.cell as usize + unit.len as usize) {
                member_cells.insert(c);
            }
        }
        let cells: Vec<usize> = (lo..hi)
            .filter(|&i| !crate::compile::is_shaper_hidden(plan.cells[i].ch))
            .filter(|&i| !member_cells.contains(&i))
            .collect();
        let mut next_cell = cells.iter().copied();
        let mut expected_cell = next_cell.next();
        for info in infos {
            let cluster = info.cluster as usize;
            let Some(cell_idx) = expected_cell else {
                result.fail(format!(
                    "block '{}': glyph at cluster {cluster} has no char left to match",
                    block.id
                ));
                break;
            };
            let cell = &plan.cells[cell_idx];
            if cluster < cell.byte_start {
                // artifact of a hidden control (own or merged cluster)
                continue;
            }
            if cluster > cell.byte_start {
                result.fail(format!(
                    "block '{}': byte {} ({:?}) produced no glyph",
                    block.id, cell.byte_start, cell.ch
                ));
                break;
            }
            let got = info.glyph_id as u16;
            let unit = unit_of_cell.get(&cell_idx);
            let expected = match unit {
                Some(unit) => {
                    result.glyphs_checked += 1;
                    if unit.pal.is_some() {
                        result.colored_glyphs += 1;
                    }
                    match unit.pal {
                        Some(pal) => alt_of
                            .get(&(unit.glyph, pal))
                            .copied()
                            .unwrap_or(unit.glyph),
                        None => unit.glyph,
                    }
                }
                // an unmapped cell: the shaper emits .notdef here (browsers
                // render it through a fallback font instead)
                None => 0,
            };
            if got != expected {
                result.fail(format!(
                    "block '{}' byte {} ({:?}): got glyph {}, expected {} ({})",
                    block.id,
                    cell.byte_start,
                    cell.ch,
                    got,
                    expected,
                    if unit.is_some_and(|u| u.pal.is_some()) {
                        "colored copy"
                    } else {
                        "base"
                    }
                ));
            }
            expected_cell = next_cell.next();
        }
        if let Some(cell_idx) = expected_cell {
            result.fail(format!(
                "block '{}': byte {} ({:?}) produced no glyph",
                block.id, plan.cells[cell_idx].byte_start, plan.cells[cell_idx].ch
            ));
        }
    }

    verify_palettes(ttf, plan, &mut result);
    result
}

/// Read COLR/CPAL back from the produced font and check every colored copy
/// under every palette (theme).
pub(crate) fn verify_palettes(ttf: &[u8], plan: &GlyphPlan, result: &mut VerifyResult) {
    let font = match FontRef::new(ttf) {
        Ok(font) => font,
        Err(e) => {
            result.fail(format!("cannot re-read produced font: {e}"));
            return;
        }
    };
    let (Some(colr), Some(cpal)) = (font.colr().ok(), font.cpal().ok()) else {
        if plan.alt_glyphs.is_empty() {
            return;
        }
        result.fail("font lacks COLR/CPAL but the plan has colored copies".into());
        return;
    };
    let empty: &[read_fonts::tables::colr::BaseGlyph] = &[];
    let base_records: &[read_fonts::tables::colr::BaseGlyph] = colr
        .base_glyph_records()
        .and_then(|r| r.ok())
        .unwrap_or(empty);
    let empty_layers: &[read_fonts::tables::colr::Layer] = &[];
    let layer_records: &[read_fonts::tables::colr::Layer] = colr
        .layer_records()
        .and_then(|r| r.ok())
        .unwrap_or(empty_layers);
    let mut painted: HashMap<u16, (u16, u16)> = HashMap::new(); // alt -> (layer glyph, slot)
    for record in base_records {
        let first = record.first_layer_index() as usize;
        if record.num_layers() as usize != 1 {
            result.fail(format!(
                "COLR base glyph {} has {} layers, expected exactly 1",
                record.glyph_id().to_u16(),
                record.num_layers()
            ));
            continue;
        }
        if let Some(layer) = layer_records.get(first) {
            painted.insert(
                record.glyph_id().to_u16(),
                (layer.glyph_id().to_u16(), layer.palette_index()),
            );
        }
    }

    let empty_colors: &[read_fonts::tables::cpal::ColorRecord] = &[];
    let colors: &[read_fonts::tables::cpal::ColorRecord] = cpal
        .color_records_array()
        .and_then(|r| r.ok())
        .unwrap_or(empty_colors);
    let num_palettes = cpal.num_palettes() as usize;
    let num_slots = cpal.num_palette_entries() as usize;
    if num_palettes != plan.theme_fgs.len() || num_slots != plan.slots.len() {
        result.fail(format!(
            "CPAL has {num_palettes} palettes x {num_slots} slots, plan expects {} x {}",
            plan.theme_fgs.len(),
            plan.slots.len()
        ));
        return;
    }
    let palette_base: Vec<usize> = cpal
        .color_record_indices()
        .iter()
        .map(|v| v.get() as usize)
        .collect();

    for theme in 0..num_palettes {
        result.palettes_checked += 1;
        // every slot's palette entry must equal the theme intent
        for (slot, vector) in plan.slots.iter().enumerate() {
            let want = vector[theme].unwrap_or(plan.theme_fgs[theme]);
            let record = palette_base
                .get(theme)
                .and_then(|base| colors.get(base + slot));
            match record {
                Some(color) => {
                    let got = (color.red(), color.green(), color.blue(), color.alpha());
                    if got != (want.red, want.green, want.blue, want.alpha) {
                        result.fail(format!(
                            "palette {theme} slot {slot}: got rgba {got:?}, theme says {:?}",
                            (want.red, want.green, want.blue, want.alpha)
                        ));
                    }
                }
                None => result.fail(format!(
                    "palette {theme} has no color record for slot {slot}"
                )),
            }
        }
        // every colored copy paints its base glyph at the right slot
        for &(base, slot, alt) in &plan.alt_glyphs {
            match painted.get(&alt) {
                Some(&(layer_gid, layer_slot)) => {
                    if layer_gid != base || layer_slot != slot {
                        result.fail(format!(
                            "COLR for glyph {alt}: paints ({layer_gid}, {layer_slot}), expected ({base}, {slot})"
                        ));
                    }
                }
                None => result.fail(format!("no COLR record for colored glyph {alt}")),
            }
        }
    }
}

/// COLR/CPAL facts read back from a produced font: the colored-copy paint
/// map and the palette colors indexed `[theme][slot]`.
struct Painted {
    /// alt glyph -> (layer glyph, slot)
    of: HashMap<u16, (u16, u16)>,
    /// `[theme][slot]` RGBA colors
    colors: Vec<Vec<(u8, u8, u8, u8)>>,
}

fn read_painted(ttf: &[u8]) -> Option<Painted> {
    let font = FontRef::new(ttf).ok()?;
    let colr = font.colr().ok()?;
    let cpal = font.cpal().ok()?;
    let empty: &[read_fonts::tables::colr::BaseGlyph] = &[];
    let base_records: &[read_fonts::tables::colr::BaseGlyph] = colr
        .base_glyph_records()
        .and_then(|r| r.ok())
        .unwrap_or(empty);
    let empty_layers: &[read_fonts::tables::colr::Layer] = &[];
    let layer_records: &[read_fonts::tables::colr::Layer] = colr
        .layer_records()
        .and_then(|r| r.ok())
        .unwrap_or(empty_layers);
    let mut of = HashMap::new();
    for record in base_records {
        let first = record.first_layer_index() as usize;
        if record.num_layers() as usize == 1 {
            if let Some(layer) = layer_records.get(first) {
                of.insert(
                    record.glyph_id().to_u16(),
                    (layer.glyph_id().to_u16(), layer.palette_index()),
                );
            }
        }
    }
    let empty_colors: &[read_fonts::tables::cpal::ColorRecord] = &[];
    let colors_1d: &[read_fonts::tables::cpal::ColorRecord] = cpal
        .color_records_array()
        .and_then(|r| r.ok())
        .unwrap_or(empty_colors);
    let num_palettes = cpal.num_palettes() as usize;
    let num_slots = cpal.num_palette_entries() as usize;
    let base: Vec<usize> = cpal
        .color_record_indices()
        .iter()
        .map(|v| v.get() as usize)
        .collect();
    let colors = (0..num_palettes)
        .map(|t| {
            (0..num_slots)
                .map(|s| {
                    colors_1d
                        .get(base[t] + s)
                        .map(|c| (c.red(), c.green(), c.blue(), c.alpha()))
                        .unwrap_or((0, 0, 0, 0))
                })
                .collect()
        })
        .collect();
    Some(Painted { of, colors })
}

/// Standalone verification: rebuild the color intents from the produced
/// font + the original inputs (blocks and themes), then run the oracle in
/// its intent-checking form (see the module docs for what it can and
/// cannot check without the source font).
///
/// The per-cell slot intents are recomputed with the produced font's
/// charmap; slot ordering is deterministic on (text, themes), so slot ids
/// match the build's even though the alt glyph ids may not.
pub fn verify_font(
    ttf: &[u8],
    blocks: &[crate::spec::Block<'_>],
    themes: &[ThemeSpec],
) -> Result<VerifyResult, BuildError> {
    let font = FontRef::new(ttf).map_err(|e| BuildError::FontParse(e.to_string()))?;
    let num_glyphs = font.maxp()?.num_glyphs();
    let num_alts = font
        .colr()
        .ok()
        .and_then(|c| c.base_glyph_records().and_then(|r| r.ok()))
        .map(|records| records.len() as u16)
        .unwrap_or(0);
    let num_source_glyphs = num_glyphs
        .checked_sub(num_alts)
        .ok_or_else(|| BuildError::FontParse("more COLR glyphs than total glyphs".into()))?;

    let ss = crate::tokenize::syntax_set();
    let charmap = skrifa::charmap::Charmap::new(&font);
    let prepared: Vec<PreparedBlock<'_>> = blocks
        .iter()
        .map(|block| {
            Ok(PreparedBlock {
                id: block.id.clone(),
                text: block.text,
                colors: crate::tokenize::colorize_block(block, themes, ss)?,
            })
        })
        .collect::<Result<_, BuildError>>()?;
    let theme_fgs: Vec<crate::spec::Color> = themes.iter().map(|t| t.foreground).collect();
    let plan = crate::compile::compile(
        &prepared,
        &|ch, vs| {
            if let Some(vs) = vs {
                if let Some(skrifa::charmap::MapVariant::Variant(g)) = charmap.map_variant(ch, vs) {
                    return Some(g.to_u32() as u16).filter(|g| *g != 0);
                }
            }
            charmap
                .map(ch)
                .map(|g| g.to_u32() as u16)
                .filter(|g| *g != 0)
        },
        num_source_glyphs,
        &theme_fgs,
        None, // no source font at hand: per-cell intents only
    )?;

    let mut result = VerifyResult::ok();
    let face = match Face::from_slice(ttf, 0) {
        Some(face) => face,
        None => {
            result.fail("rustybuzz cannot parse the produced font".into());
            return Ok(result);
        }
    };
    let Some(painted) = read_painted(ttf) else {
        if plan.slots.is_empty() {
            return Ok(result);
        }
        result.fail("font lacks COLR/CPAL but the intents color cells".into());
        return Ok(result);
    };
    let num_slots = plan.slots.len();

    for (block_idx, block) in prepared.iter().enumerate() {
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(block.text);
        let shaped = rustybuzz::shape(&face, &[], buffer);
        let infos = shaped.glyph_infos();

        // non-hidden cells of the block, in order
        let cells: Vec<usize> = plan
            .block_ranges
            .get(block_idx)
            .map(|&(lo, hi)| {
                (lo..hi)
                    .filter(|&i| !crate::compile::is_shaper_hidden(plan.cells[i].ch))
                    .collect()
            })
            .unwrap_or_default();
        // real glyphs only: a glyph is real iff its cluster is the byte
        // offset of a non-hidden cell (rustybuzz emits extra glyphs for
        // hidden controls — their clusters belong to hidden cells, which
        // never collide with a non-hidden cell's byte offset)
        let mut cell_of_cluster = HashMap::new();
        for &c in &cells {
            cell_of_cluster.insert(plan.cells[c].byte_start, c);
        }
        let real: Vec<(u16, usize)> = infos
            .iter()
            .filter_map(|info| {
                let cluster = info.cluster as usize;
                cell_of_cluster
                    .contains_key(&cluster)
                    .then_some((info.glyph_id as u16, cluster))
            })
            .collect();
        // glyph count sanity: two real glyphs may not share a cluster
        // (the produced font never decomposes), and every non-hidden cell
        // must start or sit inside exactly one glyph span
        let mut dup = false;
        for w in real.windows(2) {
            if w[0].1 == w[1].1 {
                result.fail(format!(
                    "block '{}': two glyphs share cluster {}",
                    block.id, w[0].1
                ));
                dup = true;
            }
        }
        if dup {
            continue;
        }

        // walk spans: the cells between one glyph's cluster and the next's
        // (a ligature's later member cells produce no glyph of their own)
        let block_end = plan
            .block_ranges
            .get(block_idx)
            .map(|&(_, hi)| hi)
            .unwrap_or(plan.cells.len());
        let mut covered = vec![false; plan.cells.len()];
        for (k, &(glyph, cluster)) in real.iter().enumerate() {
            let Some(&first) = cell_of_cluster.get(&cluster) else {
                continue;
            };
            let next_cluster = real.get(k + 1).map(|&(_, c)| c).unwrap_or(usize::MAX);
            let span_end = cells
                .iter()
                .find(|&&c| plan.cells[c].byte_start >= next_cluster)
                .copied()
                .unwrap_or(block_end);
            for c in covered.iter_mut().take(span_end).skip(first) {
                *c = true;
            }
            result.glyphs_checked += 1;
            // intent: uniform slot over every covered cell
            let mut intent = plan.cells[first].pal;
            for c in (first..span_end).filter(|&c| plan.cells[c].block == block_idx) {
                if plan.cells[c].pal != intent {
                    intent = None;
                    break;
                }
            }
            match intent {
                Some(slot) => {
                    result.colored_glyphs += 1;
                    match painted.of.get(&glyph) {
                        Some(&(_, painted_slot)) => {
                            if painted_slot != slot {
                                result.fail(format!(
                                    "block '{}': byte {} colors slot {slot} but the glyph \
                                     paints slot {painted_slot}",
                                    block.id, cluster
                                ));
                            }
                        }
                        None => result.fail(format!(
                            "block '{}': byte {} colors slot {slot} but the glyph is uncolored",
                            block.id, cluster
                        )),
                    }
                }
                None => {
                    if painted.of.contains_key(&glyph) {
                        result.fail(format!(
                            "block '{}': byte {} has mixed or no color but the glyph is \
                             a colored copy",
                            block.id, cluster
                        ));
                    }
                }
            }
        }
        // every non-hidden cell must sit inside some glyph span
        for &c in &cells {
            if !covered[c] {
                result.fail(format!(
                    "block '{}': byte {} ({:?}) produced no glyph",
                    block.id, plan.cells[c].byte_start, plan.cells[c].ch
                ));
            }
        }
    }

    // palette contents match the recomputed slot vectors (slot ordering is
    // deterministic, so ids line up with the build's)
    for (theme, colors) in painted.colors.iter().enumerate() {
        result.palettes_checked += 1;
        for (slot, vector) in plan.slots.iter().enumerate() {
            let want = vector
                .get(theme)
                .copied()
                .flatten()
                .unwrap_or(plan.theme_fgs[theme]);
            let want = (want.red, want.green, want.blue, want.alpha);
            if colors.get(slot).copied().unwrap_or((0, 0, 0, 0)) != want {
                result.fail(format!(
                    "palette {theme} slot {slot}: got {:?}, intents say {want:?}",
                    colors.get(slot)
                ));
            }
        }
        if colors.len() != num_slots {
            result.fail(format!(
                "palette {theme} has {} slots, intents say {num_slots}",
                colors.len()
            ));
        }
    }

    Ok(result)
}
