//! Stage 4 of the pipeline: assemble the output font from the source font
//! plus the glyph plan.
//!
//! v1 strategy: keep the source font's tables byte-for-byte where possible
//! (name, OS/2, gasp, hinting programs, ...) and rewrite only what the
//! plan touches. Exact mode subsets the glyph set to what the blocks use
//! (see [`subset_glyphs`]); the cmap is rebuilt whenever glyph ids move,
//! otherwise (and in generic mode) it is copied byte-for-byte, cmap14
//! included. The colored copies are *empty* glyphs whose COLR layer
//! references the base glyph — near-zero size cost per color. The source
//! font's own GSUB/GPOS/GDEF (and any AAT layout tables) are dropped so
//! nothing interferes with the generated `calt`; M2 adds a
//! `--keep-base-features` escape hatch.

use read_fonts::{FontRef, TableProvider};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use write_fonts::tables::cmap::{
    Cmap, Cmap14, CmapSubtable, EncodingRecord, NonDefaultUvs, PlatformId, UvsMapping,
    VariationSelector,
};
use write_fonts::tables::colr::{BaseGlyph, Colr, Layer};
use write_fonts::tables::cpal::{ColorRecord, Cpal};
use write_fonts::tables::gsub::Gsub;
use write_fonts::tables::hmtx::{Hmtx, LongMetric};
use write_fonts::tables::loca::{Loca, LocaFormat};
use write_fonts::types::{GlyphId, GlyphId16, Tag, Uint24};
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

/// Source-font tables that carry their own glyph-id references we do not
/// rewrite (embedded bitmaps, SVG outlines, an existing color layer, math
/// parts). When any is present subsetting is skipped — the full glyph set
/// ships, exactly today's behavior — instead of producing stale ids.
const SUBSET_BLOCKING_TABLES: &[Tag] = &[
    Tag::new(b"EBDT"),
    Tag::new(b"EBLC"),
    Tag::new(b"EBSC"),
    Tag::new(b"CBDT"),
    Tag::new(b"CBLC"),
    Tag::new(b"sbix"),
    Tag::new(b"SVG "),
    Tag::new(b"COLR"),
    Tag::new(b"CPAL"),
    Tag::new(b"MATH"),
];

/// Sentinel in `Subset::map` for glyphs that did not survive subsetting.
const DROPPED: u16 = u16::MAX;

/// Glyph id remapping for a subset font (exact mode only).
pub struct Subset {
    /// old gid -> new gid; `DROPPED` marks glyphs not kept.
    map: Vec<u16>,
    /// new gid -> old gid (`unmap[new] == old`), for source lookups.
    unmap: Vec<u16>,
    /// Number of surviving source glyphs.
    kept: u16,
}

/// Subset the plan's glyphs: keep .notdef, every glyph the blocks' text
/// resolved to (cells and cmap14 variants), and the transitive components
/// of composite glyphs; renumber the survivors and rewrite the plan
/// (cells, classes, alt copies, injected plains) to the new ids so every
/// downstream consumer — GSUB, COLR, hmtx, the verify oracle — sees one
/// consistent numbering.
///
/// Returns `None` when subsetting does not apply or cannot win: fonts
/// carrying other glyph-referencing tables, a keep-set covering (almost)
/// everything, or no mapped text at all. The caller then keeps the full
/// glyph set. Generic plans never subset: their font must highlight any
/// text of the language, so the whole charset stays.
pub fn subset_glyphs(
    loaded: &LoadedFont<'_>,
    plan: &mut GlyphPlan,
    variants: &mut Vec<(char, char, u16)>,
) -> Option<Subset> {
    for tag in SUBSET_BLOCKING_TABLES {
        if loaded.font.data_for_tag(*tag).is_some() {
            return None;
        }
    }

    let num = loaded.num_glyphs as usize;
    let mut used: BTreeSet<u16> = BTreeSet::new();
    used.insert(0); // .notdef always ships
    for cell in &plan.cells {
        if let Some(gid) = cell.gid {
            used.insert(gid);
        }
    }
    for &(_, _, gid) in variants.iter() {
        used.insert(gid);
    }
    if used.len() <= 1 {
        return None; // nothing maps; a subset cmap would strand every char
    }

    // composites reference their components by glyph id; keep them all
    let mut queue: Vec<u16> = used.iter().copied().collect();
    while let Some(gid) = queue.pop() {
        for component in components_of(loaded, gid) {
            if used.insert(component) {
                queue.push(component);
            }
        }
    }
    if used.len() >= num {
        return None; // nothing to drop
    }

    let mut map = vec![DROPPED; num];
    let mut unmap = Vec::with_capacity(used.len());
    for (new, &old) in used.iter().enumerate() {
        map[old as usize] = new as u16;
        unmap.push(old);
    }
    let kept = used.len() as u16;

    for cell in plan.cells.iter_mut() {
        if let Some(gid) = cell.gid {
            cell.gid = Some(map[gid as usize]);
        }
    }

    // alt copies: bases remap (they are colored cells' glyphs, always
    // kept), the (base, slot) order is restored, alt ids are reissued
    // directly after the surviving source glyphs
    let old_num_source = plan.num_source_glyphs;
    for triple in plan.alt_glyphs.iter_mut() {
        debug_assert!(map[triple.0 as usize] != DROPPED, "alt base is a kept cell glyph");
        triple.0 = map[triple.0 as usize];
    }
    plan.alt_glyphs.sort_unstable_by_key(|t| (t.0, t.1));
    let mut alt_remap: HashMap<u16, u16> = HashMap::with_capacity(plan.alt_glyphs.len());
    for (i, triple) in plan.alt_glyphs.iter_mut().enumerate() {
        let old_alt = triple.2;
        triple.2 = kept + i as u16;
        alt_remap.insert(old_alt, triple.2);
    }

    // classes are keyed by glyph id: source glyphs through the map, alt
    // copies through their reissued ids; the class values stay as they are
    plan.class_of = std::mem::take(&mut plan.class_of)
        .into_iter()
        .filter_map(|(gid, class)| {
            let new = if gid < old_num_source {
                map[gid as usize]
            } else {
                alt_remap.get(&gid).copied().unwrap_or(DROPPED)
            };
            (new != DROPPED).then_some((new, class))
        })
        .collect();

    // injected plain glyphs keep trailing after the alt copies
    let num_alt = plan.alt_glyphs.len() as u16;
    for (i, (_, gid)) in plan.plain_glyphs.iter_mut().enumerate() {
        *gid = kept + num_alt + i as u16;
    }
    plan.num_source_glyphs = kept;

    for (_, _, gid) in variants.iter_mut() {
        *gid = map[*gid as usize];
    }
    variants.sort_unstable();
    variants.dedup();

    Some(Subset { map, unmap, kept })
}

/// Byte span of one glyph inside the source glyf.
fn glyph_span(loaded: &LoadedFont<'_>, gid: u16) -> (usize, usize) {
    let g = gid as usize;
    (
        loaded.loca_offsets[g] as usize,
        loaded.loca_offsets[g + 1] as usize,
    )
}

// composite component flags (glyf spec)
const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
const WE_HAVE_A_SCALE: u16 = 0x0008;
const MORE_COMPONENTS: u16 = 0x0010;
const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0020;
const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0040;

/// Component glyph ids of a composite glyph (empty for simple/empty
/// glyphs), parsed straight from the glyf bytes.
fn components_of(loaded: &LoadedFont<'_>, gid: u16) -> Vec<u16> {
    let (a, b) = glyph_span(loaded, gid);
    let data = &loaded.glyf_data[a..b];
    if data.len() < 12 || i16::from_be_bytes([data[0], data[1]]) >= 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut pos = 10;
    loop {
        if pos + 4 > data.len() {
            break;
        }
        let flags = u16::from_be_bytes([data[pos], data[pos + 1]]);
        out.push(u16::from_be_bytes([data[pos + 2], data[pos + 3]]));
        pos += 4;
        pos += if flags & ARG_1_AND_2_ARE_WORDS != 0 { 4 } else { 2 };
        pos += if flags & WE_HAVE_A_SCALE != 0 {
            2
        } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            4
        } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
            8
        } else {
            0
        };
        if flags & MORE_COMPONENTS == 0 {
            break;
        }
    }
    out
}

/// Copy one glyph's bytes, rewriting composite component ids to the
/// subset's new numbering (simple glyphs pass through untouched).
fn subset_glyph_bytes(loaded: &LoadedFont<'_>, old: u16, map: &[u16]) -> Vec<u8> {
    let (a, b) = glyph_span(loaded, old);
    let data = &loaded.glyf_data[a..b];
    if data.len() < 12 || i16::from_be_bytes([data[0], data[1]]) >= 0 {
        return data.to_vec();
    }
    let mut out = data.to_vec();
    let mut pos = 10;
    loop {
        if pos + 4 > out.len() {
            break;
        }
        let flags = u16::from_be_bytes([out[pos], out[pos + 1]]);
        let new = map[u16::from_be_bytes([out[pos + 2], out[pos + 3]]) as usize];
        debug_assert!(new != DROPPED, "component survived the keep-set closure");
        out[pos + 2..pos + 4].copy_from_slice(&new.to_be_bytes());
        pos += 4;
        pos += if flags & ARG_1_AND_2_ARE_WORDS != 0 { 4 } else { 2 };
        pos += if flags & WE_HAVE_A_SCALE != 0 {
            2
        } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            4
        } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
            8
        } else {
            0
        };
        if flags & MORE_COMPONENTS == 0 {
            break;
        }
    }
    out
}

/// Assemble the final TTF. `subset` (exact mode, when applicable) ships
/// only the kept glyphs; `variants` are the cmap14 (base, selector, gid)
/// triples the compile resolved, re-emitted into a rebuilt cmap.
pub fn assemble(
    loaded: &LoadedFont<'_>,
    plan: &GlyphPlan,
    gsub: Option<&Gsub>,
    subset: Option<&Subset>,
    variants: &[(char, char, u16)],
) -> Result<Vec<u8>, BuildError> {
    let num_alt = plan.alt_glyphs.len();
    let num_plain = plan.plain_glyphs.len();
    // alt/plain ids trail `plan.num_source_glyphs`, which the subset path
    // has already rewritten to the surviving source glyph count
    let total_glyphs = plan.num_source_glyphs as usize + num_alt + num_plain;

    // ---- glyf (raw, padded even) + loca (extended with empty glyphs) -----
    let mut glyf: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::with_capacity(total_glyphs + 1);
    offsets.push(0);
    match subset {
        Some(sub) => {
            for new_gid in 0..sub.kept {
                let bytes = subset_glyph_bytes(loaded, sub.unmap[new_gid as usize], &sub.map);
                glyf.extend_from_slice(&bytes);
                if !glyf.len().is_multiple_of(2) {
                    glyf.push(0);
                }
                offsets.push(glyf.len() as u32);
            }
        }
        None => {
            glyf.extend_from_slice(loaded.glyf_data);
            if !glyf.len().is_multiple_of(2) {
                glyf.push(0);
            }
            // the source loca starts with its own leading 0 (loca[0] == 0
            // by spec) — skip it, the initial `offsets.push(0)` above is
            // already this font's loca[0]. Extending with all of it double
            // prefixed the offsets and shifted every source glyph one gid
            // up: composite component gids inside the verbatim glyf then
            // pointed at the wrong glyphs (self-references included), and
            // browsers rejected the whole font at OTS (0.2.2 generic-mode
            // regression; the rustybuzz oracle only diffs glyph ids and
            // never walks outlines, so it stayed green)
            offsets.extend(loaded.loca_offsets.iter().skip(1).copied());
        }
    }
    let last = *offsets.last().expect("loca is never empty");
    for _ in 0..(num_alt + num_plain) {
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
    match subset {
        Some(sub) => {
            for new_gid in 0..sub.kept {
                let (advance, lsb) = loaded.metrics[sub.unmap[new_gid as usize] as usize];
                h_metrics.push(LongMetric::new(advance, lsb));
            }
        }
        None => {
            for &(advance, lsb) in &loaded.metrics {
                h_metrics.push(LongMetric::new(advance, lsb));
            }
        }
    }
    for &(base_gid, _pal, _alt) in &plan.alt_glyphs {
        // alt bases are new (subset) ids — map back for the metric lookup.
        // an alt may copy an injected plain glyph (the newline's body
        // copies) — those are zero-width placeholders, not source glyphs
        let old_base = match subset {
            Some(sub) => sub.unmap[base_gid as usize] as usize,
            None => base_gid as usize,
        };
        let (advance, lsb) = loaded
            .metrics
            .get(old_base)
            .copied()
            .unwrap_or((0, 0));
        h_metrics.push(LongMetric::new(advance, lsb));
    }
    for _ in 0..num_plain {
        // injected glyphs are zero-width placeholders (e.g. the newline)
        h_metrics.push(LongMetric::new(0, 0));
    }
    let hmtx = Hmtx {
        h_metrics,
        left_side_bearings: Vec::new(),
    };

    // ---- post: synthesized 3.0 (no per-glyph names) ----------------------
    let post = synth_post_v3(loaded);

    // ---- cmap: rebuilt whenever glyph ids moved; raw copy (cmap14
    // included) only when nothing was subset or injected -------------------
    let rebuild_cmap = subset.is_some() || !plan.plain_glyphs.is_empty();
    let cmap = if rebuild_cmap {
        let mut mappings: BTreeMap<char, GlyphId> = BTreeMap::new();
        for (cp, gid) in skrifa::charmap::Charmap::new(&loaded.font).mappings() {
            let old = gid.to_u32() as u16;
            let new = match subset {
                Some(sub) => sub.map[old as usize],
                None => old,
            };
            if new == DROPPED {
                continue;
            }
            if let Some(ch) = char::from_u32(cp) {
                mappings.insert(ch, GlyphId::new(new as u32));
            }
        }
        for &(cp, gid) in &plan.plain_glyphs {
            if let Some(ch) = char::from_u32(cp) {
                mappings.insert(ch, GlyphId::new(gid as u32));
            }
        }
        // used variation sequences ride along as a format-14 subtable so
        // base+VS text keeps resolving to its variant glyph in the subset
        let mut records: Vec<VariationSelector> = Vec::new();
        if !variants.is_empty() {
            let mut by_selector: BTreeMap<char, Vec<(char, u16)>> = BTreeMap::new();
            for &(base, selector, gid) in variants {
                by_selector.entry(selector).or_default().push((base, gid));
            }
            for (selector, pairs) in by_selector {
                let maps = pairs
                    .into_iter()
                    .map(|(base, gid)| UvsMapping::new(Uint24::new(base as u32), gid))
                    .collect::<Vec<_>>();
                records.push(VariationSelector::new(
                    Uint24::new(selector as u32),
                    None,
                    Some(NonDefaultUvs::new(maps.len() as u32, maps)),
                ));
            }
        }
        let cmap = if mappings.is_empty() {
            // every survivor is a variant glyph: ship the (0, 5) UVS
            // record alone instead of an empty char subtable
            Cmap::new(vec![EncodingRecord::new(
                PlatformId::Unicode,
                5,
                CmapSubtable::Format14(Cmap14::new(records)),
            )])
        } else {
            let mut cmap = Cmap::from_mappings(mappings)
                .map_err(|e| BuildError::TableWrite(e.to_string()))?;
            if !records.is_empty() {
                cmap.encoding_records.push(EncodingRecord::new(
                    PlatformId::Unicode,
                    5,
                    CmapSubtable::Format14(Cmap14::new(records)),
                ));
            }
            cmap
        };
        Some(cmap)
    } else {
        None
    };

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
    if let Some(cmap) = cmap {
        builder
            .add_table(&cmap)
            .map_err(|e| BuildError::TableWrite(e.to_string()))?;
    }
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
