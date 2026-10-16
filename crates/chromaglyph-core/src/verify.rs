//! Stage 5 of the pipeline: the rustybuzz oracle.
//!
//! The produced font is shaped in-process with rustybuzz (the same shaping
//! algorithm HarfBuzz uses, which is what browsers run). For every block we
//! compare the shaped glyph stream against the intended per-character glyphs
//! (colored copy or base). Substitution is theme-independent — the same
//! rules run under every palette — so shaping is checked once, and then
//! every CPAL palette is read back and compared with the theme intent slot
//! by slot.

use std::collections::HashMap;

use read_fonts::{FontRef, TableProvider};
use rustybuzz::{Face, UnicodeBuffer};
use syntect::parsing::SyntaxSet;

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
        // hidden cell. Each non-hidden cell must line up with exactly one
        // glyph at its own byte offset.
        let cells: Vec<&crate::compile::Cell> = plan.cells[lo..hi]
            .iter()
            .filter(|c| !crate::compile::is_shaper_hidden(c.ch))
            .collect();
        let mut next_cell = cells.iter().copied();
        let mut expected_cell = next_cell.next();
        for info in infos {
            let cluster = info.cluster as usize;
            let Some(cell) = expected_cell else {
                result.fail(format!(
                    "block '{}': glyph at cluster {cluster} has no char left to match",
                    block.id
                ));
                break;
            };
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
            result.glyphs_checked += 1;
            if cell.pal.is_some() {
                result.colored_glyphs += 1;
            }
            let expected = expected_gid(cell, &alt_of);
            let got = info.glyph_id as u16;
            if got != expected {
                result.fail(format!(
                    "block '{}' byte {} ({:?}): got glyph {}, expected {} ({})",
                    block.id,
                    cell.byte_start,
                    cell.ch,
                    got,
                    expected,
                    if cell.pal.is_some() {
                        "colored copy"
                    } else {
                        "base"
                    }
                ));
            }
            expected_cell = next_cell.next();
        }
        if let Some(cell) = expected_cell {
            result.fail(format!(
                "block '{}': byte {} ({:?}) produced no glyph",
                block.id, cell.byte_start, cell.ch
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

fn expected_gid(cell: &crate::compile::Cell, alt_of: &HashMap<(u16, u16), u16>) -> u16 {
    match (cell.gid, cell.pal) {
        (Some(gid), Some(pal)) => alt_of.get(&(gid, pal)).copied().unwrap_or(gid),
        (Some(gid), None) => gid,
        (None, _) => 0, // unmapped chars shape to .notdef
    }
}

/// Standalone verification: rebuild the plan from the produced font + the
/// original inputs (blocks and themes), then run the oracle.
///
/// The alt glyph ids are recovered from COLR (every colored copy is a COLR
/// base glyph), so `chromaglyph verify` needs only the output font and the
/// same source text and themes the font was built with.
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

    let ss = SyntaxSet::load_defaults_newlines();
    let charmap = skrifa::charmap::Charmap::new(&font);
    let prepared: Vec<PreparedBlock<'_>> = blocks
        .iter()
        .map(|block| {
            Ok(PreparedBlock {
                id: block.id.clone(),
                text: block.text,
                colors: crate::tokenize::colorize_block(block, themes, &ss)?,
            })
        })
        .collect::<Result<_, BuildError>>()?;
    let theme_fgs: Vec<crate::spec::Color> = themes.iter().map(|t| t.foreground).collect();
    let plan = crate::compile::compile(
        &prepared,
        &|ch, vs| {
            if let Some(vs) = vs {
                if let Some(skrifa::charmap::MapVariant::Variant(g)) = charmap.map_variant(ch, vs)
                {
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
    )?;
    Ok(verify_plan(ttf, &prepared, &plan))
}
