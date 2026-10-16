//! Stage 4 of the pipeline: assemble the output font from the source font
//! plus the glyph plan.
//!
//! v1 strategy: keep the source font's tables byte-for-byte where possible
//! (cmap, name, OS/2, gasp, hinting programs, ...) and rewrite only what the
//! plan touches. The colored copies are *empty* glyphs whose COLR layer
//! references the base glyph — near-zero size cost per color. The source
//! font's own GSUB/GPOS/GDEF (and any AAT layout tables) are dropped so
//! nothing interferes with the generated `calt`; M2 adds a
//! `--keep-base-features` escape hatch.

use read_fonts::{FontRef, TableProvider};
use write_fonts::tables::colr::{BaseGlyph, Colr, Layer};
use write_fonts::tables::cpal::{ColorRecord, Cpal};
use write_fonts::tables::gsub::Gsub;
use write_fonts::tables::hmtx::{Hmtx, LongMetric};
use write_fonts::tables::loca::{Loca, LocaFormat};
use write_fonts::types::{GlyphId16, Tag};
use write_fonts::FontBuilder;

use crate::compile::GlyphPlan;
use crate::error::BuildError;

/// Tables dropped from the source font: layout machinery we replace or that
/// could interfere (AAT `morx` also rewrites glyphs!), per-glyph-count-carrying
/// caches, and vertical metrics (v1 is horizontal-only).
const DROP_TABLES: &[Tag] = &[
    Tag::new(b"GSUB"),
    Tag::new(b"GPOS"),
    Tag::new(b"GDEF"),
    Tag::new(b"kern"),
    Tag::new(b"morx"),
    Tag::new(b"mort"),
    Tag::new(b"kerx"),
    Tag::new(b"trak"),
    Tag::new(b"vmtx"),
    Tag::new(b"vhea"),
    Tag::new(b"VORG"),
    Tag::new(b"LTSH"),
    Tag::new(b"VDMX"),
    Tag::new(b"hdmx"),
    Tag::new(b"DSIG"),
];

/// A validated source font with the raw pieces the assembler needs.
pub struct LoadedFont<'a> {
    pub font: FontRef<'a>,
    pub num_glyphs: u16,
    /// `num_glyphs + 1` offsets from the source loca.
    pub loca_offsets: Vec<u32>,
    pub glyf_data: &'a [u8],
    /// (advance, lsb) per source glyph id.
    pub metrics: Vec<(u16, i16)>,
}

pub fn load<'a>(bytes: &'a [u8]) -> Result<LoadedFont<'a>, BuildError> {
    let font = FontRef::new(bytes).map_err(|e| BuildError::FontParse(e.to_string()))?;

    if font.data_for_tag(Tag::new(b"CFF ")).is_some()
        || font.data_for_tag(Tag::new(b"CFF2")).is_some()
    {
        return Err(BuildError::NotTrueType);
    }
    if font.data_for_tag(Tag::new(b"fvar")).is_some() {
        return Err(BuildError::VariableFont);
    }
    const REQUIRED_TABLES: &[(Tag, &str)] = &[
        (Tag::new(b"glyf"), "glyf"),
        (Tag::new(b"loca"), "loca"),
        (Tag::new(b"cmap"), "cmap"),
        (Tag::new(b"head"), "head"),
        (Tag::new(b"hhea"), "hhea"),
        (Tag::new(b"maxp"), "maxp"),
        (Tag::new(b"hmtx"), "hmtx"),
    ];
    for (tag, name) in REQUIRED_TABLES {
        if font.data_for_tag(*tag).is_none() {
            return Err(BuildError::MissingTable(name));
        }
    }

    let head = font.head()?;
    let maxp = font.maxp()?;
    let num_glyphs = maxp.num_glyphs();
    let is_long = head.index_to_loc_format() == 1;
    let loca = font.loca(is_long)?;
    let mut loca_offsets = Vec::with_capacity(num_glyphs as usize + 1);
    for i in 0..=num_glyphs as usize {
        loca_offsets.push(
            loca.get_raw(i)
                .ok_or_else(|| BuildError::FontParse(format!("loca is truncated at glyph {i}")))?,
        );
    }
    let glyf_data = font
        .data_for_tag(Tag::new(b"glyf"))
        .ok_or(BuildError::MissingTable("glyf"))?
        .as_bytes();

    let hhea = font.hhea()?;
    let num_long = hhea.number_of_h_metrics() as usize;

    let mut metrics = Vec::with_capacity(num_glyphs as usize);
    for gid in 0..num_glyphs as usize {
        metrics.push(metric_for(&font, num_long, gid));
    }

    Ok(LoadedFont {
        font,
        num_glyphs,
        loca_offsets,
        glyf_data,
        metrics,
    })
}

/// (advance, lsb) for glyph `gid` from the source hmtx.
fn metric_for(font: &FontRef<'_>, num_long: usize, gid: usize) -> (u16, i16) {
    let hmtx = font.hmtx().expect("hmtx presence checked in load()");
    if gid < num_long {
        let m = hmtx
            .h_metrics()
            .get(gid)
            .expect("h_metrics covers numberOfHMetrics entries");
        (m.advance(), m.side_bearing())
    } else {
        let advance = hmtx.h_metrics().last().map(|m| m.advance()).unwrap_or(0);
        let lsb = hmtx
            .left_side_bearings()
            .get(gid - num_long)
            .map(|v| v.get())
            .unwrap_or(0);
        (advance, lsb)
    }
}

/// Assemble the final TTF.
pub fn assemble(
    loaded: &LoadedFont<'_>,
    plan: &GlyphPlan,
    gsub: Option<&Gsub>,
) -> Result<Vec<u8>, BuildError> {
    let num_alt = plan.alt_glyphs.len();
    let total_glyphs = loaded.num_glyphs as usize + num_alt;

    // ---- glyf (raw, padded even) + loca (extended with empty glyphs) -----
    let mut glyf = loaded.glyf_data.to_vec();
    if !glyf.len().is_multiple_of(2) {
        glyf.push(0);
    }
    let mut offsets = loaded.loca_offsets.clone();
    let last = *offsets.last().expect("loca is never empty");
    for _ in 0..num_alt {
        offsets.push(last);
    }
    let loca = Loca::new(offsets);
    let loca_is_long = matches!(loca.format(), LocaFormat::Long);

    // ---- head / hhea / maxp: raw bytes with one field patched ------------
    let head = patch_table(&loaded.font, b"head", |bytes| {
        bytes[50..52].copy_from_slice(&(loca_is_long as u16).to_be_bytes());
    });
    let hhea = patch_table(&loaded.font, b"hhea", |bytes| {
        bytes[34..36].copy_from_slice(&(total_glyphs as u16).to_be_bytes());
    });
    let maxp = patch_table(&loaded.font, b"maxp", |bytes| {
        bytes[4..6].copy_from_slice(&(total_glyphs as u16).to_be_bytes());
    });

    // ---- hmtx: long metrics for every glyph (alts copy their base) -------
    let mut h_metrics: Vec<LongMetric> = Vec::with_capacity(total_glyphs);
    for &(advance, lsb) in &loaded.metrics {
        h_metrics.push(LongMetric::new(advance, lsb));
    }
    for &(base_gid, _pal, _alt) in &plan.alt_glyphs {
        let (advance, lsb) = loaded.metrics[base_gid as usize];
        h_metrics.push(LongMetric::new(advance, lsb));
    }
    let hmtx = Hmtx {
        h_metrics,
        left_side_bearings: Vec::new(),
    };

    // ---- post: synthesized 3.0 (no per-glyph names) ----------------------
    let post = synth_post_v3(loaded);

    // ---- COLR v0 + CPAL ----------------------------------------------------
    let mut base_glyphs: Vec<BaseGlyph> = Vec::with_capacity(num_alt);
    let mut layers: Vec<Layer> = Vec::with_capacity(num_alt);
    for (i, &(base, pal, alt)) in plan.alt_glyphs.iter().enumerate() {
        base_glyphs.push(BaseGlyph::new(GlyphId16::new(alt), i as u16, 1));
        layers.push(Layer::new(GlyphId16::new(base), pal));
    }
    let colr = Colr {
        num_base_glyph_records: base_glyphs.len() as u16,
        base_glyph_records: Some(base_glyphs).into(),
        layer_records: Some(layers).into(),
        num_layer_records: num_alt as u16,
        ..Default::default()
    };
    let num_slots = plan.slots.len() as u16;
    let num_themes = plan.theme_fgs.len() as u16;
    // color records are palette-major: theme t occupies
    // [t*num_slots, (t+1)*num_slots)
    let mut color_records: Vec<ColorRecord> = Vec::with_capacity((num_slots * num_themes) as usize);
    for (theme, fg) in plan.theme_fgs.iter().enumerate() {
        for slot in &plan.slots {
            let c = slot[theme].unwrap_or(*fg);
            color_records.push(ColorRecord::new(c.blue, c.green, c.red, c.alpha));
        }
    }
    let cpal = Cpal {
        num_palette_entries: num_slots,
        num_palettes: num_themes,
        num_color_records: color_records.len() as u16,
        color_records_array: Some(color_records).into(),
        color_record_indices: (0..num_themes).map(|t| t * num_slots).collect(),
        ..Default::default()
    };

    // ---- assemble ---------------------------------------------------------
    let mut builder = FontBuilder::new();
    let add = |builder: &mut FontBuilder<'_>, tag: Tag, bytes: Vec<u8>| {
        builder.add_raw(tag, bytes);
    };
    add(&mut builder, Tag::new(b"head"), head);
    add(&mut builder, Tag::new(b"hhea"), hhea);
    add(&mut builder, Tag::new(b"maxp"), maxp);
    add(&mut builder, Tag::new(b"post"), post);
    add(&mut builder, Tag::new(b"glyf"), glyf);
    builder
        .add_table(&loca)
        .map_err(|e| BuildError::TableWrite(e.to_string()))?;
    builder
        .add_table(&hmtx)
        .map_err(|e| BuildError::TableWrite(e.to_string()))?;
    if num_alt > 0 {
        builder
            .add_table(&colr)
            .map_err(|e| BuildError::TableWrite(e.to_string()))?;
        builder
            .add_table(&cpal)
            .map_err(|e| BuildError::TableWrite(e.to_string()))?;
    }
    if let Some(gsub) = gsub {
        builder
            .add_table(gsub)
            .map_err(|e| BuildError::TableWrite(e.to_string()))?;
    }
    // copy everything else the source had, minus dropped/rewritten tables
    for record in loaded.font.table_directory().table_records() {
        let tag = record.tag();
        if builder.contains(tag) || DROP_TABLES.contains(&tag) {
            continue;
        }
        if let Some(data) = loaded.font.data_for_tag(tag) {
            builder.add_raw(tag, data.as_bytes().to_vec());
        }
    }
    Ok(builder.build())
}

/// Copy a raw table from the source font and apply a byte patch.
fn patch_table(font: &FontRef<'_>, tag: &[u8; 4], patch: impl FnOnce(&mut [u8])) -> Vec<u8> {
    let mut bytes = font
        .data_for_tag(Tag::new(tag))
        .expect("required tables checked in load()")
        .as_bytes()
        .to_vec();
    patch(&mut bytes);
    bytes
}

/// post table version 3.0 (32 bytes, no glyph names), carrying over the
/// source font's italic angle / underline metrics.
fn synth_post_v3(loaded: &LoadedFont<'_>) -> Vec<u8> {
    let (italic_angle, underline_position, underline_thickness, is_fixed) = match loaded.font.post()
    {
        Ok(post) => (
            post.italic_angle().to_bits(),
            post.underline_position().to_i16() as u16,
            post.underline_thickness().to_i16() as u16,
            post.is_fixed_pitch(),
        ),
        Err(_) => (0i32, 0u16, 0u16, 1u32),
    };
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(&0x0003_0000u32.to_be_bytes()); // version 3.0
    out.extend_from_slice(&italic_angle.to_be_bytes());
    out.extend_from_slice(&underline_position.to_be_bytes());
    out.extend_from_slice(&underline_thickness.to_be_bytes());
    out.extend_from_slice(&is_fixed.to_be_bytes());
    out.extend_from_slice(&[0u8; 16]); // memory hints, all zero
    out
}
