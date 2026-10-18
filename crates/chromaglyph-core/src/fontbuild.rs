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
//! font's own GSUB/GPOS/GDEF (and any AAT layout tables) are dropped:
//! coloring (and, since 0.2.3, ligature restoration) happens through the
//! generated `calt` alone — see `compile.rs` for how the source font's
//! shaping is re-materialized at build time instead.
//!
//! Outlines: TrueType (`glyf`/`loca`) and CFF1 (`CFF `, name-keyed) are
//! accepted, static or variable-TrueType (pinned to the default instance —
//! the glyf outlines *are* the default, so the variation machinery is
//! simply dropped). CFF2 outlines would need real instancing and are
//! rejected. WOFF1 containers and `.ttc` collections are unwrapped to a
//! plain sfnt before this stage (see [`normalize_input`]).

use read_fonts::{FontRef, TableProvider};
use std::borrow::Cow;
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

use crate::cff;
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

/// Variation machinery dropped when a variable TrueType font is pinned to
/// its default instance (see [`LoadedFont::variable`]). STAT is on the
/// list too: without `fvar` it describes axes the font no longer has.
const VARIATION_TABLES: &[Tag] = &[
    Tag::new(b"fvar"),
    Tag::new(b"avar"),
    Tag::new(b"cvar"),
    Tag::new(b"gvar"),
    Tag::new(b"HVAR"),
    Tag::new(b"VVAR"),
    Tag::new(b"MVAR"),
    Tag::new(b"STAT"),
];

/// The outline representation of the source font. TrueType glyphs are
/// sliced out of `glyf` via `loca`; CFF glyphs live in a CharStrings
/// INDEX the assembler appends empty `endchar` entries to (see `cff.rs`).
pub enum Outlines<'a> {
    TrueType {
        /// `num_glyphs + 1` offsets from the source loca.
        loca_offsets: Vec<u32>,
        glyf: &'a [u8],
    },
    Cff {
        table: &'a [u8],
        layout: cff::Layout,
    },
}

/// A validated source font with the raw pieces the assembler needs.
pub struct LoadedFont<'a> {
    pub font: FontRef<'a>,
    pub num_glyphs: u16,
    pub outlines: Outlines<'a>,
    /// (advance, lsb) per source glyph id.
    pub metrics: Vec<(u16, i16)>,
    /// True when the source carried variation tables; the output font is
    /// pinned to the default instance (a warning says so in the report).
    pub variable: bool,
    /// Load-time notes surfaced as build warnings (report + CLI).
    pub warnings: Vec<String>,
}

/// Normalize raw input bytes to a plain sfnt: unwrap WOFF1 containers,
/// extract one font out of a `.ttc` collection, pass everything else
/// through untouched.
pub fn normalize_input<'a>(bytes: &'a [u8], font_index: u32) -> Result<Cow<'a, [u8]>, BuildError> {
    match bytes.get(..4) {
        Some(b"wOFF") => Ok(Cow::Owned(crate::woff::unwrap(bytes)?)),
        Some(b"ttcf") => extract_ttc_member(bytes, font_index).map(Cow::Owned),
        _ => {
            if let Some([0, 1, 0, 0]) | Some(b"OTTO") | Some(b"true") | Some(b"ttcf") =
                bytes.get(..4)
            {
                Ok(Cow::Borrowed(bytes))
            } else {
                Err(BuildError::FontParse(
                    "not a font (expected sfnt, OTTO, WOFF or ttcf magic)".into(),
                ))
            }
        }
    }
}

/// Pull member `font_index` out of a `.ttc` collection as a standalone
/// sfnt.
///
/// A slice is never enough: table offsets inside a member's directory are
/// absolute positions in the TTC file, and Apple's collections (Menlo,
/// every system `.ttc`) keep the member directories packed at the front
/// while the table data they point at is shared at the back. So the
/// member is *relocated*: header + directory re-emitted, each table's
/// data copied from its absolute position, offsets recomputed (checksums
/// ride along unchanged — the bytes they cover did).
fn extract_ttc_member(bytes: &[u8], font_index: u32) -> Result<Vec<u8>, BuildError> {
    let bad = |msg: &str| BuildError::FontParse(format!("ttcf: {msg}"));
    let be32 = |at: usize| -> Option<u32> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
    };
    let be16 = |at: usize| -> Option<u16> {
        bytes
            .get(at..at + 2)
            .map(|b| u16::from_be_bytes(b.try_into().unwrap()))
    };
    let num_fonts = be32(8).ok_or_else(|| bad("header truncated"))?;
    if font_index >= num_fonts {
        return Err(bad(&format!(
            "font index {font_index} is out of range: the collection has {num_fonts} font(s)"
        )));
    }
    let dir_at =
        be32(12 + 4 * font_index as usize).ok_or_else(|| bad("offsets truncated"))? as usize;
    match bytes.get(dir_at..dir_at + 4) {
        Some([0, 1, 0, 0]) | Some(b"OTTO") | Some(b"true") => {}
        _ => return Err(bad("member does not look like a font")),
    }
    let num_tables = be16(dir_at + 4).ok_or_else(|| bad("member header truncated"))? as usize;
    if num_tables == 0 || num_tables > u16::MAX as usize {
        return Err(bad("member has no tables"));
    }

    struct Entry {
        tag: [u8; 4],
        checksum: u32,
        abs_offset: u32,
        length: u32,
    }
    let mut entries = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let at = dir_at + 12 + 16 * i;
        let raw = bytes
            .get(at..at + 16)
            .ok_or_else(|| bad("member directory truncated"))?;
        let entry = Entry {
            tag: raw[0..4].try_into().unwrap(),
            checksum: u32::from_be_bytes(raw[4..8].try_into().unwrap()),
            abs_offset: u32::from_be_bytes(raw[8..12].try_into().unwrap()),
            length: u32::from_be_bytes(raw[12..16].try_into().unwrap()),
        };
        if entry.abs_offset as u64 + entry.length as u64 > bytes.len() as u64 {
            return Err(bad("table data out of bounds"));
        }
        entries.push(entry);
    }

    // same layout the woff unwrapper emits: 12-byte header + 16-byte
    // records + 4-aligned table data; searchRange fields copied verbatim
    // (the table count did not change)
    let mut out = Vec::with_capacity(bytes.len() - dir_at.min(bytes.len()));
    out.extend_from_slice(&bytes[dir_at..dir_at + 12]);
    let mut table_at = (12 + 16 * num_tables) as u32;
    for entry in &entries {
        out.extend_from_slice(&entry.tag);
        out.extend_from_slice(&entry.checksum.to_be_bytes());
        out.extend_from_slice(&table_at.to_be_bytes());
        out.extend_from_slice(&entry.length.to_be_bytes());
        table_at += entry.length;
        table_at += (4 - entry.length % 4) % 4;
    }
    for entry in &entries {
        let start = entry.abs_offset as usize;
        let end = start + entry.length as usize;
        out.extend_from_slice(&bytes[start..end]);
        out.extend(std::iter::repeat_n(
            0u8,
            ((4 - entry.length % 4) % 4) as usize,
        ));
    }
    Ok(out)
}

pub fn load<'a>(bytes: &'a [u8]) -> Result<LoadedFont<'a>, BuildError> {
    let font = FontRef::new(bytes).map_err(|e| BuildError::FontParse(e.to_string()))?;

    let has_cff2 = font.data_for_tag(Tag::new(b"CFF2")).is_some();
    if has_cff2 {
        return Err(BuildError::UnsupportedFont(
            "CFF2 outlines (variable OTF) cannot be pinned without instancing; \
             build from the static OTF or the TTF instead"
                .into(),
        ));
    }
    let cff_data = font.data_for_tag(Tag::new(b"CFF "));
    let outlines =
        if let Some(data) = cff_data {
            let table = data.as_bytes();
            let layout = cff::parse(table)?;
            Outlines::Cff { table, layout }
        } else {
            const REQUIRED_TABLES: &[(Tag, &str)] = &[
                (Tag::new(b"glyf"), "glyf"),
                (Tag::new(b"loca"), "loca"),
                (Tag::new(b"cmap"), "cmap"),
            ];
            for (tag, name) in REQUIRED_TABLES {
                if font.data_for_tag(*tag).is_none() {
                    if font.data_for_tag(Tag::new(b"EBDT")).is_some()
                        || font.data_for_tag(Tag::new(b"CBDT")).is_some()
                    {
                        return Err(BuildError::UnsupportedFont(
                            "bitmap-only fonts have no outlines to color".into(),
                        ));
                    }
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
                loca_offsets.push(loca.get_raw(i).ok_or_else(|| {
                    BuildError::FontParse(format!("loca is truncated at glyph {i}"))
                })?);
            }
            let glyf = font
                .data_for_tag(Tag::new(b"glyf"))
                .ok_or(BuildError::MissingTable("glyf"))?
                .as_bytes();
            Outlines::TrueType { loca_offsets, glyf }
        };

    const SHARED_TABLES: &[(Tag, &str)] = &[
        (Tag::new(b"cmap"), "cmap"),
        (Tag::new(b"head"), "head"),
        (Tag::new(b"hhea"), "hhea"),
        (Tag::new(b"maxp"), "maxp"),
        (Tag::new(b"hmtx"), "hmtx"),
    ];
    for (tag, name) in SHARED_TABLES {
        if font.data_for_tag(*tag).is_none() {
            return Err(BuildError::MissingTable(name));
        }
    }

    let maxp = font.maxp()?;
    let num_glyphs = maxp.num_glyphs();
    if let Outlines::Cff { layout, .. } = &outlines {
        if layout.num_glyphs != num_glyphs as usize {
            return Err(BuildError::FontParse(format!(
                "maxp says {num_glyphs} glyphs, CFF CharStrings has {}",
                layout.num_glyphs
            )));
        }
    }

    // variable TrueType: the glyf outlines *are* the default instance, so
    // the font ships static at default once the variation tables are
    // dropped (see VARIATION_TABLES). Variable CFF would be CFF2, already
    // rejected above.
    let mut warnings = Vec::new();
    let variable = font.data_for_tag(Tag::new(b"fvar")).is_some();
    if variable {
        if cff_data.is_some() {
            return Err(BuildError::UnsupportedFont(
                "CFF font with fvar (variable OTF) is not supported; \
                 use the static OTF or the TTF"
                    .into(),
            ));
        }
        warnings.push(
            "variable font pinned to its default instance \
             (variation tables dropped)"
                .into(),
        );
    }

    let hhea = font.hhea()?;
    let num_long = hhea.number_of_h_metrics() as usize;

    let mut metrics = Vec::with_capacity(num_glyphs as usize);
    for gid in 0..num_glyphs as usize {
        metrics.push(metric_for(&font, num_long, gid));
    }

    Ok(LoadedFont {
        font,
        num_glyphs,
        outlines,
        metrics,
        variable,
        warnings,
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
    // CFF subsetting means renumbering a CharStrings INDEX (and charset,
    // FDSelect, ...) — v1 ships the full glyph set for CFF sources
    // instead, exactly like the blocking tables above
    if matches!(loaded.outlines, Outlines::Cff { .. }) {
        return None;
    }

    let num = loaded.num_glyphs as usize;
    let mut used: BTreeSet<u16> = BTreeSet::new();
    used.insert(0); // .notdef always ships
                    // the glyphs the output font produces: plain cells and restored
                    // substitution outputs (ligature glyphs have no cmap entry of their
                    // own — the units are what keep them in the subset)
    for unit in &plan.units {
        used.insert(unit.glyph);
    }
    // ligature rule inputs: member glyphs the nested lookups match on
    for site in &plan.lig_sites {
        for &g in &site.components {
            used.insert(g);
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

    // units carry the glyphs the output font produces (they seeded the
    // keep-set, so every one survives); ligature sites carry the pristine
    // member glyphs and the restored output the same way
    for unit in plan.units.iter_mut() {
        unit.glyph = map[unit.glyph as usize];
        debug_assert!(unit.glyph != DROPPED, "unit glyphs seeded the keep-set");
    }
    for site in plan.lig_sites.iter_mut() {
        for g in site.components.iter_mut() {
            *g = map[*g as usize];
        }
        site.out = map[site.out as usize];
    }

    // alt copies: bases remap (they are colored cells' glyphs, always
    // kept), the (base, slot) order is restored, alt ids are reissued
    // directly after the surviving source glyphs
    let old_num_source = plan.num_source_glyphs;
    for triple in plan.alt_glyphs.iter_mut() {
        debug_assert!(
            map[triple.0 as usize] != DROPPED,
            "alt base is a kept cell glyph"
        );
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
    let Outlines::TrueType { loca_offsets, .. } = &loaded.outlines else {
        unreachable!("composites are only walked on TrueType sources")
    };
    let g = gid as usize;
    (loca_offsets[g] as usize, loca_offsets[g + 1] as usize)
}

/// The source glyf bytes (TrueType sources only).
fn glyf_of<'a>(loaded: &LoadedFont<'a>) -> &'a [u8] {
    match &loaded.outlines {
        Outlines::TrueType { glyf, .. } => glyf,
        Outlines::Cff { .. } => unreachable!("CFF sources never walk glyf"),
    }
}

// composite component flags, as every implementation actually numbers
// them (Apple's TrueType spec; OTS/Chrome, FreeType and fonttools agree —
// see the 0.2.3 regression): MORE_COMPONENTS is 0x0020, not 0x0010
const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
const WE_HAVE_A_SCALE: u16 = 0x0008;
const MORE_COMPONENTS: u16 = 0x0020;
const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;

/// Component glyph ids of a composite glyph (empty for simple/empty
/// glyphs), parsed straight from the glyf bytes.
fn components_of(loaded: &LoadedFont<'_>, gid: u16) -> Vec<u16> {
    let (a, b) = glyph_span(loaded, gid);
    let data = &glyf_of(loaded)[a..b];
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
        pos += if flags & ARG_1_AND_2_ARE_WORDS != 0 {
            4
        } else {
            2
        };
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
    let data = &glyf_of(loaded)[a..b];
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
        pos += if flags & ARG_1_AND_2_ARE_WORDS != 0 {
            4
        } else {
            2
        };
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

    // ---- outlines: TrueType re-emits glyf+loca (raw, padded even, with
    // the empty alt/plain glyphs appended as zero-length entries); CFF
    // re-lays-out its table with the new glyphs as endchar charstrings ---
    let mut glyf: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::with_capacity(total_glyphs + 1);
    let mut cff_table: Option<Vec<u8>> = None;
    offsets.push(0);
    match (&loaded.outlines, subset) {
        (Outlines::TrueType { .. }, Some(sub)) => {
            for new_gid in 0..sub.kept {
                let bytes = subset_glyph_bytes(loaded, sub.unmap[new_gid as usize], &sub.map);
                glyf.extend_from_slice(&bytes);
                if !glyf.len().is_multiple_of(2) {
                    glyf.push(0);
                }
                offsets.push(glyf.len() as u32);
            }
        }
        (Outlines::TrueType { loca_offsets, .. }, None) => {
            glyf.extend_from_slice(glyf_of(loaded));
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
            offsets.extend(loca_offsets.iter().skip(1).copied());
        }
        (Outlines::Cff { table, layout }, _) => {
            cff_table = Some(cff::append_empty_glyphs(
                table,
                layout,
                num_alt + num_plain,
            )?);
        }
    }
    if cff_table.is_none() {
        // the alt/plain glyphs ride as zero-length entries at the tail of
        // the source (or subset) loca
        let last = *offsets.last().expect("loca is never empty");
        for _ in 0..(num_alt + num_plain) {
            offsets.push(last);
        }
    }
    let loca = Loca::new(offsets);
    let loca_is_long = matches!(loca.format(), LocaFormat::Long);

    // ---- head / hhea / maxp: raw bytes with one field patched ------------
    // (head's indexToLocFormat only exists for TrueType; a CFF font's head
    // passes through untouched)
    let head = patch_table(&loaded.font, b"head", |bytes| {
        if cff_table.is_none() {
            bytes[50..52].copy_from_slice(&(loca_is_long as u16).to_be_bytes());
        }
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
        let (advance, lsb) = loaded.metrics.get(old_base).copied().unwrap_or((0, 0));
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
            let mut cmap =
                Cmap::from_mappings(mappings).map_err(|e| BuildError::TableWrite(e.to_string()))?;
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
    match cff_table {
        // CFF source: the extended CFF table replaces glyf/loca, and
        // FontBuilder emits the OTTO flavor for a CFF-bearing table set
        Some(table) => add(&mut builder, Tag::new(b"CFF "), table),
        None => {
            add(&mut builder, Tag::new(b"glyf"), glyf);
            builder
                .add_table(&loca)
                .map_err(|e| BuildError::TableWrite(e.to_string()))?;
        }
    }
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
    // (and, for a pinned variable source, the variation machinery)
    for record in loaded.font.table_directory().table_records() {
        let tag = record.tag();
        if builder.contains(tag)
            || DROP_TABLES.contains(&tag)
            || (loaded.variable && VARIATION_TABLES.contains(&tag))
        {
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
