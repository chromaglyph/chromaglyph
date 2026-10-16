//! Stage 5 of the pipeline: the rustybuzz oracle.
//!
//! The produced font is shaped in-process with rustybuzz (the same shaping
//! algorithm HarfBuzz uses, which is what browsers run). For every block we
//! compare the shaped glyph stream against the intended per-character glyphs
//! (colored copy or base), then read COLR/CPAL back out of the produced font
//! and check that each colored copy paints the base glyph in exactly the
//! theme color.

use std::collections::HashMap;

use read_fonts::{FontRef, TableProvider};
use rustybuzz::{Face, UnicodeBuffer};

use crate::compile::{GlyphPlan, PreparedBlock};
use crate::error::BuildError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifyResult {
    pub ok: bool,
    pub glyphs_checked: usize,
    pub colored_glyphs: usize,
    pub failures: Vec<String>,
}

impl VerifyResult {
    pub fn ok() -> Self {
        VerifyResult {
            ok: true,
            glyphs_checked: 0,
            colored_glyphs: 0,
            failures: Vec::new(),
        }
    }

    fn fail(&mut self, msg: String) {
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
        let cells = &plan.cells[lo..hi];
        if infos.len() != cells.len() {
            result.fail(format!(
                "block '{}': shaped {} glyphs for {} chars",
                block.id,
                infos.len(),
                cells.len()
            ));
            if infos.is_empty() {
                continue;
            }
        }
        let expected: Vec<u16> = cells
            .iter()
            .map(|cell| expected_gid(cell, &alt_of))
            .collect();
        for (i, cell) in cells.iter().enumerate() {
            result.glyphs_checked += 1;
            if cell.pal.is_some() {
                result.colored_glyphs += 1;
            }
            let Some(info) = infos.get(i) else { continue };
            if info.cluster as usize != cell.byte_start {
                result.fail(format!(
                    "block '{}' byte {}: cluster {} != {}",
                    block.id, cell.byte_start, info.cluster, cell.byte_start
                ));
            }
            let got = info.glyph_id as u16;
            if got != expected[i] {
                result.fail(format!(
                    "block '{}' byte {} ({:?}): got glyph {}, expected {} ({})",
                    block.id,
                    cell.byte_start,
                    cell.ch,
                    got,
                    expected[i],
                    if cell.pal.is_some() {
                        "colored copy"
                    } else {
                        "base"
                    }
                ));
            }
        }
    }

    verify_colors(ttf, plan, &mut result);
    result
}

/// Read COLR/CPAL back from the produced font and check every colored copy.
fn verify_colors(ttf: &[u8], plan: &GlyphPlan, result: &mut VerifyResult) {
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
    let mut painted: HashMap<u16, (u16, u16)> = HashMap::new(); // alt -> (layer glyph, palette idx)
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
    for &(base, pal, alt) in &plan.alt_glyphs {
        match painted.get(&alt) {
            Some(&(layer_gid, layer_pal)) => {
                if layer_gid != base || layer_pal != pal {
                    result.fail(format!(
                        "COLR for glyph {alt}: paints ({layer_gid}, {layer_pal}), expected ({base}, {pal})"
                    ));
                }
                match colors.get(pal as usize) {
                    Some(color) => {
                        let got = (color.red(), color.green(), color.blue(), color.alpha());
                        let want = plan.palette[pal as usize];
                        if got != (want.red, want.green, want.blue, want.alpha) {
                            result.fail(format!(
                                "CPAL[{pal}]: got rgba {got:?}, theme says {:?}",
                                (want.red, want.green, want.blue, want.alpha)
                            ));
                        }
                    }
                    None => result.fail(format!("CPAL has no color record {pal}")),
                }
            }
            None => result.fail(format!("no COLR record for colored glyph {alt}")),
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

/// Standalone verification: rebuild the plan from the produced font alone.
///
/// The alt glyph ids are recovered from COLR (every colored copy is a COLR
/// base glyph), so `chromaglyph verify` needs only the output font and the
/// original input text.
pub fn verify_font(
    ttf: &[u8],
    blocks: &[PreparedBlock<'_>],
    theme: &crate::spec::Theme,
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

    let charmap = skrifa::charmap::Charmap::new(&font);
    let plan = crate::compile::compile(
        blocks,
        theme,
        &|ch| {
            charmap
                .map(ch)
                .map(|g| g.to_u32() as u16)
                .filter(|g| *g != 0)
        },
        num_source_glyphs,
    )?;
    Ok(verify_plan(ttf, blocks, &plan))
}
