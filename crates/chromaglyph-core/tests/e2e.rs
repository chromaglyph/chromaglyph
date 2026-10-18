//! End-to-end: fixture TTF + code snippet + two themes → one color font
//! with two CPAL palettes, verified by the rustybuzz oracle (inside
//! `build`), then re-verified standalone from the woff bytes.

use chromaglyph_core::theme::ThemeSpec;
use chromaglyph_core::{build, verify, Block, BuildInput, OutputFormat};
use read_fonts::TableProvider;

const FONT: &[u8] = include_bytes!("fixtures/JetBrainsMono-Regular.ttf");

const SAMPLE: &str = r#"// chromaglyph m2
fn main() {
    let msg = "hello chromaglyph";
    let n = 42;
    println!("{} {}", msg, n);
}
"#;

fn blocks(text: &'static str) -> Vec<Block<'static>> {
    vec![Block {
        id: "main".into(),
        lang: "rust".into(),
        text,
    }]
}

fn themes() -> Vec<ThemeSpec> {
    vec![
        ThemeSpec::builtin("dark-plus").unwrap(),
        ThemeSpec::builtin("light-plus").unwrap(),
    ]
}

#[test]
fn two_themes_one_font() {
    let themes = themes();
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("build + inline verify must succeed");

    assert!(out.report.rules > 0, "expected some substitution rules");
    assert!(out.report.alt_glyphs > 0);
    assert_eq!(out.report.slots, out.report.palettes[0].len());
    assert_eq!(out.report.palettes.len(), 2, "one palette per theme");
    assert!(out.report.verify.ok);
    assert_eq!(out.report.verify.palettes_checked, 2);
    assert_eq!(out.report.verify.failures.len(), 0);

    // the palettes actually differ (dark vs light for the same slot)
    assert_ne!(out.report.palettes[0], out.report.palettes[1]);

    for table in ["GSUB", "COLR", "CPAL", "glyf", "loca", "hmtx"] {
        assert!(
            out.report.tables.contains_key(table),
            "missing table {table}: {:?}",
            out.report.tables.keys().collect::<Vec<_>>()
        );
    }

    // the woff round-trips: run the standalone oracle from the woff bytes
    let result = verify(&out.woff, &blocks(SAMPLE), &themes).expect("standalone verify");
    assert!(
        result.ok,
        "standalone verify from woff: {:?}",
        result.failures
    );
}

#[test]
fn single_theme_still_works() {
    let themes = vec![ThemeSpec::builtin("dark-plus").unwrap()];
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("single-theme build");
    assert_eq!(out.report.palettes.len(), 1);
    assert!(out.report.verify.ok);
}

#[test]
fn cff_otf_builds_and_verifies() {
    let themes = themes();
    let out = build(&BuildInput {
        font_bytes: include_bytes!("fixtures/SourceCodePro-Regular.otf"),
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("CFF (OTF) source must build");

    // the output stays CFF-flavored with a rebuilt, subset CFF table
    assert_eq!(&out.ttf[0..4], b"OTTO", "CFF sources keep the OTTO flavor");
    assert!(out.report.tables.contains_key("CFF "), "CFF table present");
    assert!(
        !out.report.tables.contains_key("glyf"),
        "no glyf in a CFF build"
    );
    // exact mode subsets CFF like it subsets TrueType: the charstrings
    // of unused glyphs are dropped, not shipped
    assert!(
        out.report.total_glyphs < out.report.source_glyphs / 2,
        "CFF glyph set collapses: {} of {}",
        out.report.total_glyphs,
        out.report.source_glyphs
    );
    assert!(out.report.verify.ok, "{:?}", out.report.verify.failures);

    // the produced CFF re-parses with read-fonts' own parser and its
    // charset covers every glyph that ships (subset + appended empties)
    let font = read_fonts::FontRef::new(&out.ttf).unwrap();
    let cff = font.cff().unwrap();
    let charset = cff.charset(0).unwrap();
    assert_eq!(
        charset.num_glyphs() as usize,
        out.report.total_glyphs as usize
    );

    // woff round-trip: the wrapped font carries the OTTO flavor through
    assert_eq!(&out.woff[4..8], b"OTTO");
    let result = verify(&out.woff, &blocks(SAMPLE), &themes).expect("standalone verify");
    assert!(result.ok, "{:?}", result.failures);
}

#[test]
fn woff2_output_builds_and_verifies() {
    let themes = themes();
    let sample_blocks = blocks(SAMPLE);
    let input = |format| BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &sample_blocks,
        run_verify: true,
        generic: None,
        font_name: Some("out.woff2"),
        semantic: None,
        ligatures: false,
        format,
    };
    let w1 = build(&input(OutputFormat::Woff)).expect("woff1 build");
    let out = build(&input(OutputFormat::Woff2)).expect("woff2 build");

    assert_eq!(&out.woff[0..4], b"wOF2");
    assert!(
        out.woff.len() < w1.woff.len(),
        "woff2 {} must undercut woff1 {} on the same build",
        out.woff.len(),
        w1.woff.len()
    );
    // the CSS names the container that actually ships
    assert!(out.css.contains("format('woff2')"), "{}", out.css);

    // decode (wuff, the same oracle the input path trusts) and compare
    // every table against the raw sfnt — the container is lossless
    let decoded = wuff::decompress_woff2(&out.woff).expect("wuff decodes our woff2");
    let a = read_fonts::FontRef::new(&out.ttf).unwrap();
    let b = read_fonts::FontRef::new(&decoded).unwrap();
    let tags: Vec<read_fonts::types::Tag> = a
        .table_directory()
        .table_records()
        .iter()
        .map(|r| r.tag())
        .collect();
    assert_eq!(tags.len(), b.table_directory().table_records().len());
    for tag in tags {
        let (mut x, mut y) = (
            a.data_for_tag(tag).unwrap().as_bytes().to_vec(),
            b.data_for_tag(tag).unwrap().as_bytes().to_vec(),
        );
        // whole-font checksum: decoders recompute it on reconstruction
        if tag == read_fonts::types::Tag::new(b"head") && x.len() >= 12 {
            x[8..12].fill(0);
            y[8..12].fill(0);
        }
        assert_eq!(x, y, "table {tag} changed through the woff2 roundtrip");
    }

    // the standalone oracle runs from the woff2 bytes themselves
    let result = verify(&out.woff, &blocks(SAMPLE), &themes).expect("standalone verify");
    assert!(result.ok, "{:?}", result.failures);
}

#[test]
fn woff2_cff_output_rides_as_null_transform() {
    let themes = themes();
    let out = build(&BuildInput {
        font_bytes: include_bytes!("fixtures/SourceCodePro-Regular.otf"),
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: Some("out.woff2"),
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff2,
    })
    .expect("CFF-flavored output wraps WOFF2 directly (ttf2woff2 >= 0.14.1)");

    // a real WOFF2 container carrying the OTTO flavor through, with the
    // CSS naming it and no fallback warning anywhere
    assert_eq!(&out.woff[0..4], b"wOF2");
    assert_eq!(&out.woff[4..8], b"OTTO", "the CFF flavor survives the wrap");
    assert!(out.css.contains("format('woff2')"), "{}", out.css);
    assert!(
        !out.report.warnings.iter().any(|w| w.contains("WOFF")),
        "no container fallback anymore: {:?}",
        out.report.warnings
    );

    // decode and compare every table against the raw sfnt — CFF rides
    // null-transform, so the roundtrip is lossless like TrueType's
    let decoded = wuff::decompress_woff2(&out.woff).expect("wuff decodes our CFF woff2");
    assert_eq!(&decoded[0..4], b"OTTO");
    let a = read_fonts::FontRef::new(&out.ttf).unwrap();
    let b = read_fonts::FontRef::new(&decoded).unwrap();
    let tags: Vec<read_fonts::types::Tag> = a
        .table_directory()
        .table_records()
        .iter()
        .map(|r| r.tag())
        .collect();
    assert_eq!(tags.len(), b.table_directory().table_records().len());
    for tag in tags {
        // whole-font checksum: decoders recompute it on reconstruction
        let (mut x, mut y) = (
            a.data_for_tag(tag).unwrap().as_bytes().to_vec(),
            b.data_for_tag(tag).unwrap().as_bytes().to_vec(),
        );
        if tag == read_fonts::types::Tag::new(b"head") && x.len() >= 12 {
            x[8..12].fill(0);
            y[8..12].fill(0);
        }
        assert_eq!(x, y, "table {tag} changed through the woff2 roundtrip");
    }

    let result = verify(&out.woff, &blocks(SAMPLE), &themes).expect("standalone verify");
    assert!(result.ok, "{:?}", result.failures);
}

#[test]
fn sfnt_format_ships_no_container() {
    let themes = themes();
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: Some("out.ttf"),
        semantic: None,
        ligatures: false,
        format: OutputFormat::Sfnt,
    })
    .expect("sfnt build");

    assert!(out.woff.is_empty(), "no container is computed for sfnt");
    assert!(out.css.contains("format('truetype')"), "{}", out.css);
    let result = verify(&out.ttf, &blocks(SAMPLE), &themes).expect("standalone verify");
    assert!(result.ok, "{:?}", result.failures);
}

#[test]
fn variable_ttf_is_pinned_to_default() {
    let themes = themes();
    let out = build(&BuildInput {
        font_bytes: include_bytes!("fixtures/JetBrainsMono-wght-VF.ttf"),
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("variable TrueType source must build at its default instance");

    // the glyf outlines *are* the default instance; shipping them means
    // no variation machinery may survive into the output
    for table in [
        "fvar", "gvar", "avar", "cvar", "HVAR", "VVAR", "MVAR", "STAT",
    ] {
        assert!(
            !out.report.tables.contains_key(table),
            "variation table {table} survived: {:?}",
            out.report.tables.keys().collect::<Vec<_>>()
        );
    }
    assert!(
        out.report
            .warnings
            .iter()
            .any(|w| w.contains("pinned to its default instance")),
        "warnings must say the font was pinned: {:?}",
        out.report.warnings
    );
    assert!(out.report.verify.ok, "{:?}", out.report.verify.failures);
}

#[test]
fn woff_input_is_unwrapped() {
    let themes = themes();
    let woff = chromaglyph_core::woff::wrap(FONT).expect("wrap fixture");
    let out = build(&BuildInput {
        font_bytes: &woff,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("WOFF1 input must unwrap and build");
    assert!(out.report.verify.ok, "{:?}", out.report.verify.failures);
}

#[test]
fn woff2_input_decodes_and_builds() {
    let themes = themes();
    // a real Google Fonts woff2 (JetBrains Mono latin): brotli tables +
    // the transformed glyf/loca streams, the common shape of "a font
    // downloaded from the web"
    let woff2 = include_bytes!("fixtures/JetBrainsMono-latin.woff2");
    assert_eq!(&woff2[0..4], b"wOF2");
    let out = build(&BuildInput {
        font_bytes: woff2,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("WOFF2 input must decode and build");
    assert!(out.report.verify.ok, "{:?}", out.report.verify.failures);

    // the decoded font goes through the normal TrueType pipeline
    assert!(out.report.tables.contains_key("glyf"));
    let result = verify(&out.woff, &blocks(SAMPLE), &themes).expect("standalone verify");
    assert!(result.ok, "{:?}", result.failures);
}

/// A minimal `.ttc` collection: header + offsets + (aligned) font copies.
/// A `.ttc` with each member stored as its own sfnt. Table offsets inside
/// a member's directory are **file-absolute** in a collection (that is
/// how shared-data layouts work — see `synthesize_shared_ttc`), so each
/// embedded font's offsets are rebased by the member position.
fn synthesize_ttc(font: &[u8], copies: usize) -> Vec<u8> {
    let num_tables = u16::from_be_bytes([font[4], font[5]]) as usize;
    let header_len = 12 + 4 * copies;
    let mut out = Vec::new();
    out.extend_from_slice(b"ttcf");
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(copies as u32).to_be_bytes());
    let mut body = Vec::new();
    let mut offsets = Vec::new();
    for _ in 0..copies {
        let member_at = header_len + body.len();
        offsets.push(member_at as u32);
        let mut member = font.to_vec();
        for i in 0..num_tables {
            let at = 12 + 16 * i;
            let orig = u32::from_be_bytes(font[at + 8..at + 12].try_into().unwrap());
            member[at + 8..at + 12].copy_from_slice(&((member_at as u32) + orig).to_be_bytes());
        }
        let pad = (4 - member.len() % 4) % 4;
        body.extend_from_slice(&member);
        body.extend(std::iter::repeat_n(0u8, pad));
    }
    for offset in offsets {
        out.extend_from_slice(&offset.to_be_bytes());
    }
    out.extend_from_slice(&body);
    out
}

#[test]
fn ttc_input_extracts_the_indexed_font() {
    let themes = themes();
    let ttc = synthesize_ttc(FONT, 2);
    let out = build(&BuildInput {
        font_bytes: &ttc,
        font_index: 1,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("ttc input must extract the requested member");
    assert!(out.report.verify.ok, "{:?}", out.report.verify.failures);

    // out-of-range index is a load error, not a wrong-font build
    let err = match build(&BuildInput {
        font_bytes: &ttc,
        font_index: 2,
        themes: &themes,
        blocks: &blocks("fn"),
        run_verify: false,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    }) {
        Ok(_) => panic!("out-of-range font index must be rejected"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("out of range"), "got: {err}");
}

/// Apple's `.ttc` layout (every macOS system collection): member
/// directories packed at the front, the table data they point at shared
/// at the back through file-absolute offsets — a member slice would be
/// directories only.
fn synthesize_shared_ttc(font: &[u8], members: usize) -> Vec<u8> {
    let num_tables = u16::from_be_bytes([font[4], font[5]]) as usize;
    let dir_len = 12 + 16 * num_tables;
    let data_at = 12 + 4 * members + dir_len * members;

    let mut out = Vec::new();
    out.extend_from_slice(b"ttcf");
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(members as u32).to_be_bytes());
    for k in 0..members {
        let member_at = (12 + 4 * members + k * dir_len) as u32;
        out.extend_from_slice(&member_at.to_be_bytes());
    }
    for _ in 0..members {
        out.extend_from_slice(&font[..12]); // flavor + search assists
        for i in 0..num_tables {
            let at = 12 + 16 * i;
            out.extend_from_slice(&font[at..at + 8]); // tag + checksum
            let orig = u32::from_be_bytes(font[at + 8..at + 12].try_into().unwrap());
            out.extend_from_slice(&((data_at + orig as usize - dir_len) as u32).to_be_bytes());
            out.extend_from_slice(&font[at + 12..at + 16]); // length
        }
    }
    out.extend_from_slice(&font[dir_len..]);
    out
}

#[test]
fn ttc_apple_layout_shared_data_extracts() {
    let themes = themes();
    let ttc = synthesize_shared_ttc(FONT, 3);
    for index in [0u32, 2] {
        let out = build(&BuildInput {
            font_bytes: &ttc,
            font_index: index,
            themes: &themes,
            blocks: &blocks(SAMPLE),
            run_verify: true,
            generic: None,
            font_name: None,
            semantic: None,
            ligatures: false,
            format: OutputFormat::Woff,
        })
        .expect("Apple-layout ttcf member {index} must extract");
        assert!(
            out.report.verify.ok,
            "member {index}: {:?}",
            out.report.verify.failures
        );
    }
}

#[test]
fn cff_ligatures_rematerialize_in_otf_builds() {
    let themes = themes();
    let src = "let x = a != b; // != -> www\n";
    let out = build(&BuildInput {
        font_bytes: include_bytes!("fixtures/SourceCodePro-Regular.otf"),
        font_index: 0,
        themes: &themes,
        blocks: &blocks(src),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: true,
        format: OutputFormat::Woff,
    })
    .expect("ligature shaping must work against a CFF source");
    assert!(out.report.verify.ok, "{:?}", out.report.verify.failures);
}

#[test]
fn cross_language_ambiguity_degrades_not_fails() {
    let themes = themes();
    let blocks = vec![
        Block {
            id: "rust-block".into(),
            lang: "rust".into(),
            text: "let",
        },
        Block {
            id: "plain-block".into(),
            lang: "plain".into(),
            text: "let",
        },
    ];
    // rust colors "let" as a keyword, plain leaves it uncolored; identical
    // text in both blocks means any rule would fire in both — the build
    // degrades the occurrence (warning) instead of failing
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks,
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("build never fails on ambiguity");
    assert!(
        !out.report.warnings.is_empty(),
        "degradation must be reported"
    );
    assert!(out.report.verify.ok, "degraded font must still verify");
}

#[test]
fn real_tokenization_colors_function_names() {
    // syntect resolves scopes Dark+ styles: `main` (entity.name.function)
    // gets the function yellow, distinct from keywords — meaning the demo
    // is no longer limited to the M1 lexer's five scopes
    let themes = vec![ThemeSpec::builtin("dark-plus").unwrap()];
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: false,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .unwrap();
    assert!(
        out.report.palettes[0].iter().any(|c| c == "#dcdcaa"),
        "function yellow in palette"
    );
}

/// Code blocks with emoji + variation selectors: the VS chars produce no
/// glyph (the shaper drops them), the emoji fall back to another font at
/// run time, and coloring must continue around both.
#[test]
fn emoji_and_variation_selectors_shape_and_color() {
    const EMOJI_SAMPLE: &str = "// ship it \u{1F680}\u{FE0F} today\nlet s = \"\u{1F44D}\u{FE0F}\";\nlet t = \"x\u{FE0F}y\";\n";
    let themes = themes();
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(EMOJI_SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("emoji sample must build + verify");

    // the mapped text around the emoji keeps its color: the string "xy"
    // around the bare VS16 is one colored run (the VS is transparent)
    assert!(out.report.colored_cells >= 4, "coloring survives the emoji");
    // degradations may mention the emoji (no glyph) but never the VS
    for warning in &out.report.warnings {
        assert!(
            !warning.contains("variation"),
            "VS must not be treated as a missing-glyph char: {warning}"
        );
    }
    assert!(out.report.verify.ok);
}

/// A source font with a cmap format 14 (UVS) subtable: `base + VS16` must
/// resolve to the variant glyph both at compile time (the cell carries the
/// UVS glyph id) and in the oracle (rustybuzz resolves via the cmap14 that
/// passes through to the output font).
#[test]
fn cmap14_variation_sequence_resolves_to_variant_glyph() {
    let font = inject_cmap14_uvs(FONT, '\u{0078}', '\u{FE0F}'); // "x" + VS16
    const UVS_SAMPLE: &str = "let s = \"x\u{FE0F}\";\n";
    let themes = themes();
    let out = build(&BuildInput {
        font_bytes: &font,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(UVS_SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("UVS sample must build + verify (oracle resolves cmap14)");
    assert!(out.report.verify.ok);
    assert!(out.report.colored_cells > 0);
}

/// Splice a cmap format 14 subtable into `font` mapping
/// (`base`, `selector`) -> a glyph that differs from the base's default
/// mapping, so a correct pipeline is observable.
fn inject_cmap14_uvs(font: &[u8], base: char, selector: char) -> Vec<u8> {
    use read_fonts::{FontRef, TableProvider};

    let parsed = FontRef::new(font).expect("fixture parses");
    let num_glyphs = parsed.maxp().unwrap().num_glyphs();
    let charmap = skrifa::charmap::Charmap::new(&parsed);
    let default_gid = charmap.map(base).expect("base is mapped").to_u32() as u16;
    let variant_gid = (default_gid + 1) % num_glyphs; // observably different

    // format 14 subtable: header + one varSelector record + one
    // non-default UVS mapping (record is 3 + 4 + 4 = 11 bytes)
    let record_len: u32 = 3 + 4 + 4;
    let non_default_off: u32 = 2 + 4 + 4 + record_len;
    let length = non_default_off + 4 + 3 + 2;
    let mut sub: Vec<u8> = Vec::new();
    sub.extend_from_slice(&14u16.to_be_bytes()); // format
    sub.extend_from_slice(&length.to_be_bytes());
    sub.extend_from_slice(&1u32.to_be_bytes()); // numVarSelectorRecords
                                                // record: varSelector (uint24), defaultUVS offset (0), nonDefaultUVS
                                                // offset (relative to subtable start, right after the record)
    sub.extend_from_slice(&(selector as u32).to_be_bytes()[1..4]);
    sub.extend_from_slice(&0u32.to_be_bytes());
    sub.extend_from_slice(&non_default_off.to_be_bytes());
    // nonDefaultUVS: numUVSMappings + {unicodeValue uint24, glyphID uint16}
    sub.extend_from_slice(&1u32.to_be_bytes());
    sub.extend_from_slice(&(base as u32).to_be_bytes()[1..4]);
    sub.extend_from_slice(&variant_gid.to_be_bytes());
    assert_eq!(sub.len(), length as usize);

    // rebuild the cmap table: header + old records + (0,5) record + subtables
    let old = parsed
        .data_for_tag(read_fonts::types::Tag::new(b"cmap"))
        .expect("cmap present")
        .as_bytes();
    let (version, num_tables) = (
        u16::from_be_bytes([old[0], old[1]]),
        u16::from_be_bytes([old[2], old[3]]) as usize,
    );
    let old_subtables_off = 4 + num_tables * 8;
    // one extra 8-byte record shifts every existing subtable by 8 bytes;
    // their table-relative offsets must be patched accordingly
    let mut records: Vec<u8> = old[4..old_subtables_off].to_vec();
    for rec in records.chunks_exact_mut(8) {
        let off = u32::from_be_bytes([rec[4], rec[5], rec[6], rec[7]]) + 8;
        rec[4..8].copy_from_slice(&off.to_be_bytes());
    }
    // `sub` lands after the header, the n+1 records, and the old subtables
    let new_sub_off = (old.len() + 8) as u32;
    records.extend_from_slice(&0u16.to_be_bytes()); // platform 0
    records.extend_from_slice(&5u16.to_be_bytes()); // encoding 5 = UVS
    records.extend_from_slice(&new_sub_off.to_be_bytes());
    let mut cmap = Vec::with_capacity(old.len() + sub.len() + 8);
    cmap.extend_from_slice(&version.to_be_bytes());
    cmap.extend_from_slice(&((num_tables + 1) as u16).to_be_bytes());
    cmap.extend_from_slice(&records);
    cmap.extend_from_slice(&old[old_subtables_off..]);
    cmap.extend_from_slice(&sub);

    // splice the bigger cmap back into the font: simplest correct way is to
    // let write-fonts re-assemble a directory over all tables
    let mut builder = write_fonts::FontBuilder::new();
    for record in parsed.table_directory().table_records() {
        let tag = record.tag();
        if tag == read_fonts::types::Tag::new(b"cmap") {
            builder.add_raw(tag, cmap.clone());
        } else {
            builder.add_raw(tag, parsed.data_for_tag(tag).unwrap().as_bytes().to_vec());
        }
    }
    builder.build()
}

/// Generic mode: one font that highlights ANY Rust text (content unknown
/// at build time) — keywords, line comments, strings — verified by the
/// rustybuzz oracle against the reference lexer on curated + random
/// samples.
#[test]
fn generic_mode_highlights_arbitrary_text() {
    let themes = themes();
    let spec = chromaglyph_core::generic::rust();
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &[],
        run_verify: true,
        generic: Some(&spec),
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("generic build + oracle over random samples");

    assert!(out.report.rules >= 100, "expected a real rule machine");
    assert_eq!(out.report.palettes.len(), 2, "one palette per theme");
    assert!(
        out.report.alt_glyphs > 1000,
        "body-state copies for the charset"
    );
    assert!(out.report.verify.ok, "{:?}", out.report.verify.failures);
}

/// Adversarial samples beyond the build-time ones: every state transition
/// order, unterminated states, adjacent states, keyword-boundary soup.
#[test]
fn generic_mode_adversarial_samples() {
    adversarial(
        &chromaglyph_core::generic::rust(),
        &[
            "// only comment",
            "\"only string\"",
            "fn//c\n\"s\"let",
            "\"unterminated string let fn",
            "// unterminated comment let fn",
            "letletlet let_ let1 1let",
            "\"a\"\"b\"\"c\"",
            "///doc\n//!inner\n////",
            "x = \"//\" / \"/\" // \"/\"\n/=\"\"",
        ],
    );
}

/// The v1.2 languages: multi-quote strings, template literals spanning
/// lines, Python triple quotes — each built with the oracle running over
/// curated + deterministic-random samples inside `build`.
#[test]
fn generic_mode_new_languages() {
    let themes = themes();
    for (spec, min_rules) in [
        (chromaglyph_core::generic::javascript(), 250),
        (chromaglyph_core::generic::typescript(), 260),
        (chromaglyph_core::generic::python(), 200),
        (chromaglyph_core::generic::go(), 180),
        (chromaglyph_core::generic::java(), 250),
        (chromaglyph_core::generic::c(), 300),
        (chromaglyph_core::generic::cpp(), 380),
        (chromaglyph_core::generic::csharp(), 380),
        (chromaglyph_core::generic::php(), 300),
        (chromaglyph_core::generic::ruby(), 220),
        (chromaglyph_core::generic::lua(), 220),
        (chromaglyph_core::generic::shell(), 220),
        (chromaglyph_core::generic::html(), 180),
        (chromaglyph_core::generic::css(), 1400),
    ] {
        let out = build(&BuildInput {
            font_bytes: FONT,
            font_index: 0,
            themes: &themes,
            blocks: &[],
            run_verify: true,
            generic: Some(&spec),
            font_name: None,
            semantic: None,
            ligatures: false,
            format: OutputFormat::Woff,
        })
        .unwrap_or_else(|e| panic!("{}: generic build + oracle: {e}", spec.name));
        assert!(
            out.report.rules >= min_rules,
            "{}: expected a real rule machine, got {}",
            spec.name,
            out.report.rules
        );
        assert_eq!(
            out.report.palettes.len(),
            2,
            "{}: one palette per theme",
            spec.name
        );
        assert!(
            out.report.verify.ok,
            "{}: {:?}",
            spec.name, out.report.verify.failures
        );
    }
}

/// Hand-picked nastiness per new language: quote-kind interleaving,
/// template literals across lines, triple-quote precedence, `$` words,
/// f-strings, line-start keywords over the injected newline.
#[test]
fn generic_mode_adversarial_new_languages() {
    adversarial(
        &chromaglyph_core::generic::javascript(),
        &[
            "'only single'",
            "`only template`",
            "`a\nb\nc`",
            "let s = `he said \"hi\" and 'bye'`;",
            "'a`b' `c'd' e`",
            "'''''' ``````",
            "let//c\n`s`const",
            "`unterminated template let const",
            "// `unterminated` comment let const",
            "functionfunction function_ _function 1function",
            "x = '`' / \"`\" // '`'\n`=`\"\"",
            "$let let$ $let$ $$let$$ let$$",
            "\nreturn at_line_start\nconst at_line_start\n42 at_line_start",
            "`${x /* not a comment */}` + '// str'",
            "await\nawait awaitx xawait",
        ],
    );
    adversarial(
        &chromaglyph_core::generic::python(),
        &[
            "\"\"\"only triple\"\"\"",
            "'''only single triple'''",
            "\"\"\"a\nb\nc\"\"\"",
            "\"\"\"' and \" inside\"\"\"",
            "'''\"\"\"x\"\"\"'''",
            "\"\"\"\"\"\"\"\"\"\"",
            "def# c\n\"\"\"s\"\"\"class",
            "\"\"\"unterminated triple def class",
            "# unterminated comment def if",
            "defdefdef def_ _def 1def",
            "s = '\"' # '\"'\nx = \"\"\"'#'\"\"\"",
            "f'{x}' f\"{y}\" f'''{z}'''",
            "\nreturn at_line_start\ndef at_line_start\n42 at_line_start",
            "\"\"\"${x // not a comment}\"\"\" + '# str'",
            "lambda\nlambda lambdax xlambda",
        ],
    );
    adversarial(
        &chromaglyph_core::generic::c(),
        &[
            "'only char'",
            "\"only string\"",
            "char q = '\\''; char b = '\\\\';",
            "char *s = \"a\\\"b\"; char t = 'x';",
            "'a\"b' \"c'd' e\"",
            "'' \"\"",
            "return//c\nint",
            "\"unterminated string return int",
            "// 'unterminated' comment return int",
            "/* unterminated block return int",
            "returnreturn return_ _return 1return",
            "x = '\"' / \"'\" // \"'\nq = '\\\"'",
            "#include \"file.h\" // include undef",
            "ifdef include undef define pragma",
            "\nreturn at_line_start\nint at_line_start\n42 at_line_start",
            "sizeof\nsizeof sizex xsizeof",
        ],
    );
    adversarial(
        &chromaglyph_core::generic::cpp(),
        &[
            "'only char'",
            "\"only string\"",
            "char q = '\\''; char b = '\\\\';",
            "char *s = \"a\\\"b\"; char t = 'x';",
            "'a\"b' \"c'd' e\"",
            "'' \"\"",
            "return//c\nint",
            "\"unterminated string return int",
            "// 'unterminated' comment return int",
            "/* unterminated block return int",
            "classclass class_ _class 1class",
            // keywords with embedded digits: the digit glyph carries the
            // keyword class, and still opens numbers after a boundary
            "char8_tchar8_t char8_t_ _char8_t 1char8_t",
            "char16_t x; char32_t y; char8_t z = 8;",
            "8 48 x8 8x 0x18 42 8;",
            "x = '\"' / \"'\" // \"'\nq = '\\\"'",
            "#include \"file.h\" // include undef",
            "namespace template typename operator public private",
            "\nreturn at_line_start\nclass at_line_start\n42 at_line_start",
            "nullptr\nnullptr nullptrx xnullptr",
        ],
    );
    adversarial(
        &chromaglyph_core::generic::csharp(),
        &[
            "'only char'",
            "\"only string\"",
            "char q = '\\''; char b = '\\\\';",
            "string s = \"a\\\"b\"; char t = 'x';",
            "'a\"b' \"c'd' e\"",
            "'' \"\"@\"\" $\"\"",
            "return//c\nint",
            "\"unterminated string return int",
            "// 'unterminated' comment return int",
            "/* unterminated block return int",
            "classclass class_ _class 1class",
            "@\"unterminated verbatim return int",
            "@\"He said \"\"hi\"\" today\" + \"ok\"",
            "string v = @\"back\\slash \"is\" literal\";",
            "string i = $\"sum = {a + b} done\";",
            "x = '\"' / \"'\" // \"'\nq = '\\\"'",
            "namespace using nameof typeof await",
            "\nreturn at_line_start\nclass at_line_start\n42 at_line_start",
            "nameof\nnameof nameofx xnameof",
            "@return return @return@ @\"x\"",
        ],
    );
    adversarial(
        &chromaglyph_core::generic::ruby(),
        &[
            "'only single'",
            "\"only double\"",
            "\"a\\\"b\" 'c\\'d'",
            "\"nested #{\"inner\"} out\"",
            "'it\\'s' \"a\\\"b\"",
            "'' \"\" #{\"x\"}",
            "return# c\ndef",
            "\"unterminated string def if",
            "# unterminated comment def if",
            "endend end_ _end 1end",
            "x = 1 # 'quote' \"double\"\ny = 2",
            "class module def yield self",
            "\nreturn at_line_start\ndef at_line_start\n42 at_line_start",
            "self\nself selfx xself",
        ],
    );
    adversarial(
        &chromaglyph_core::generic::php(),
        &[
            "'only single'",
            "\"only double\"",
            "$a = \"d\\\"b\"; $b = 'c\\'d';",
            "\"hole {$x} and {$y[0]} out\"",
            "'no {$interp} here' . \"plain $var stays\"",
            "'' \"\" {$x} $x",
            "return//c\nfunction",
            "\"unterminated string function return",
            "// 'unterminated' comment function return",
            "/* unterminated block function return",
            "functionfunction function_ _function 1function",
            "$function function$ $function$ $1 $2",
            "x = '\"' / \"'\" // \"'\nq = '\\\"'",
            "echo print require include instanceof",
            "\nreturn at_line_start\nfunction at_line_start\n42 at_line_start",
        ],
    );
    adversarial(
        &chromaglyph_core::generic::lua(),
        &[
            "'only single'",
            "\"only double\"",
            "s = \"a\\\"b\"; c = 'x\\'y';",
            "long = [[has 'quotes\" and -- stuff]]",
            "[[unterminated long function",
            "--[[ unterminated block function",
            "'' \"\" [[]] --[[x]] --[[]",
            "return--c\nlocal",
            "\"unterminated string local return",
            "-- 'unterminated' comment local return",
            "endend end_ _end 1end",
            "x = 1 -- 'q' \"d\"\ny = 2",
            "function local nil repeat until",
            "\nreturn at_line_start\nlocal at_line_start\n42 at_line_start",
        ],
    );
    adversarial(
        &chromaglyph_core::generic::shell(),
        &[
            "'only single raw'",
            "\"only double\"",
            "e=\"a\\\"b\"; c='x'y'",
            "echo \"home ${HOME} and ${x:-dflt}\"",
            "'single $notinterp \\- literal'",
            "'' \"\" ${x} ${}",
            "return#c\nfunction",
            "\"unterminated string function return",
            "# unterminated comment function return",
            "esacesac esac_ _esac 1esac",
            "x=1 # 'q' \"d\"\ny=2",
            "for while done esac select in",
            "$1 $2 ${1} ${x}1 1$",
            "for f in *.txt; do echo \"$f\"; done",
            "\nfunction at_line_start\n42 at_line_start",
        ],
    );
    adversarial(
        &chromaglyph_core::generic::html(),
        &[
            "<div>only text</div>",
            "<p class=\"a\">x</p>",
            "<img src='x.png' alt=\"y\"/>",
            "<div a='has \"dq\" in'>t</div>",
            "<div a=\"has 'sq' in\">t</div>",
            "<input type=checkbox disabled>",
            "<!-- comment with <div class=\"x\"> inside -->",
            "&amp; &lt; &#39; &no-end",
            "a < b, c > d, e <f g",
            "<3 <> <4u </> < /div>",
            "<my-widget data-x=\"1\">",
            "<div\n  class=\"next line\">plain",
            "<div =x b=\"c\">",
            "<div a<b=\"c\">",
            "text \"quotes\" stay plain",
            "<div><span><a href=\"x\">deep</a></span></div>",
            "\n<div at_line_start\n&amp; at_line_start",
        ],
    );
    adversarial(
        &chromaglyph_core::generic::css(),
        &[
            "a { color: red; }",
            ".a:hover { color: blue; }",
            "background:hover",
            "li:nth-child(2) { top: 0; }",
            "/* c */ .x { /* d */ y: z; }",
            "p { content: \"a; b\"; }",
            "p { font: 12px/1.5 sans-serif; }",
            "m { prop:value;compact:true }",
            "x { y: z",
            "grid-template-columns: repeat(3, 1fr);",
            "@media (min-width: 600px) { .b { padding: 4px 8px; } }",
            "@import url('a.css');",
            ":root { --gap: 8px; }",
            ".a .b > .c + .d ~ .e { top: 0; }",
            "sel { width: 50% !important; }",
            "width: calc(100% - 8px);",
            "{ ; } ; {",
            "color:\nred;",
            "\ncolor at_line_start\ndiv at_line_start\n42 at_line_start",
            "color: red; display: flex; gap: 8px",
        ],
    );
}

/// Shared adversarial harness: build without the oracle, then shape the
/// font over the samples and diff against the reference lexer.
fn adversarial(spec: &chromaglyph_core::generic::LanguageSpec, samples: &[&str]) {
    let themes = themes();
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &[],
        run_verify: false,
        generic: Some(spec),
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("generic build");

    // recover (base, slot) -> alt from the produced font's COLR layers
    use std::collections::HashMap;
    let font = read_fonts::FontRef::new(&out.ttf).unwrap();
    use read_fonts::TableProvider;
    let colr = font.colr().unwrap();
    let mut alt_of: HashMap<(u16, u16), u16> = HashMap::new();
    let layers: Vec<(u16, u16)> = colr
        .layer_records()
        .and_then(|r| r.ok())
        .unwrap_or_default()
        .iter()
        .map(|l| (l.glyph_id().to_u16(), l.palette_index()))
        .collect();
    for record in colr
        .base_glyph_records()
        .and_then(|r| r.ok())
        .unwrap_or_default()
    {
        let alt = record.glyph_id().to_u16();
        if let Some(&(base, slot)) = layers.get(record.first_layer_index() as usize) {
            alt_of.insert((base, slot), alt);
        }
    }
    // model the produced font (the injected newline is mapped)
    let charmap = skrifa::charmap::Charmap::new(&font);
    let samples: Vec<String> = samples.iter().map(|s| s.to_string()).collect();
    let result = chromaglyph_core::generic::verify_generic(
        &out.ttf,
        spec,
        &alt_of,
        &|ch| {
            charmap
                .map(ch)
                .map(|g| g.to_u32() as u16)
                .filter(|g| *g != 0)
        },
        &samples,
    );
    assert!(result.ok, "{}: {:?}", spec.name, result.failures);
}

/// Exact-mode TypeScript: syntect's default syntax set cannot resolve
/// `ts` (it silently fell back to plain text — zero coloring, zero
/// warnings); the two-face superset fixes it. TSX rides along.
#[test]
fn exact_mode_highlights_typescript() {
    let themes = themes();
    let src: &str = "interface User {\n    readonly id: number\n    name: string\n}\n\nfunction greet(u: User): string {\n    return `hi ${u.name}`\n}\n";
    for lang in ["ts", "typescript", "tsx"] {
        let out = build(&BuildInput {
            font_bytes: FONT,
            font_index: 0,
            themes: &themes,
            blocks: &[Block {
                id: "ts".into(),
                lang: lang.into(),
                text: src,
            }],
            run_verify: true,
            generic: None,
            font_name: None,
            semantic: None,
            ligatures: false,
            format: OutputFormat::Woff,
        })
        .unwrap_or_else(|e| panic!("{lang}: {e}"));
        assert!(
            out.report.colored_cells > 40,
            "{lang}: expected real highlighting, got {} colored cells",
            out.report.colored_cells
        );
        assert!(
            out.report.verify.ok,
            "{lang}: {:?}",
            out.report.verify.failures
        );
    }
}

/// CJK text in a block: the font has no glyphs for it, so it renders via
/// browser fallback and cannot be highlighted — but the mapped text
/// around it (the `//` opener, the trailing newline barrier) keeps
/// coloring, the oracle stays green, and the build warning names the
/// actual missing characters plus the CJK hint.
#[test]
fn cjk_text_is_a_barrier_and_the_warning_names_it() {
    let themes = themes();
    let src: &str = "fn main() {\n    let v = 0.2;   // 数字状态：小数点\n}\n";
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(src),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("CJK sample builds and verifies");
    assert!(out.report.verify.ok, "{:?}", out.report.verify.failures);
    assert!(
        out.report.colored_cells > 0,
        "the mapped parts around the barrier keep coloring"
    );
    let cjk_warning = out
        .report
        .warnings
        .iter()
        .find(|w| w.contains("CJK"))
        .expect("a CJK hint is attached");
    assert!(cjk_warning.contains("fallback"), "{cjk_warning}");
    let missing = out
        .report
        .warnings
        .iter()
        .find(|w| w.contains("Missing:"))
        .expect("the missing characters are listed");
    assert!(
        missing.contains('\u{6570}'),
        "the actual CJK chars appear: {missing}"
    );
}

/// The semantic hook: spans of (byte range, dotted scope) overlay the
/// syntactic colors. A fake producer stands in for chromaglyph-treesitter
/// (core stays free of C dependencies); the real crate runs the same hook.
#[test]
fn semantic_layer_overlays_syntactic_colors() {
    let themes = themes();
    let plain = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: false,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .unwrap();
    let semantic =
        |block: &chromaglyph_core::Block<'_>| -> Vec<(usize, usize, String)> {
            block
                .text
                .match_indices("msg")
                .map(|(pos, found)| (pos, pos + found.len(), "variable.parameter".to_string()))
                .chain(block.text.match_indices("main").map(|(pos, found)| {
                    (pos, pos + found.len(), "entity.name.function".to_string())
                }))
                .collect()
        };
    let layered = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: Some(&semantic),
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("layered build + oracle");

    assert!(layered.report.verify.ok);
    assert!(
        layered.report.colored_cells > plain.report.colored_cells,
        "the semantic layer colors chars the syntax layer leaves plain: {} vs {}",
        layered.report.colored_cells,
        plain.report.colored_cells
    );
    // the parameter blue is a palette entry now
    assert!(
        layered.report.palettes[0].iter().any(|c| c == "#9cdcfe"),
        "{:?}",
        layered.report.palettes
    );
}

/// Exact mode subsets the font to the glyphs the blocks use: the produced
/// glyph count collapses while every char of the sample still resolves in
/// the rebuilt cmap, and the oracle (which shapes the subset font itself)
/// stays green. The jetbrains fixture carries ~650 glyphs; the sample
/// touches a few dozen.
#[test]
fn exact_mode_subsets_the_font() {
    let themes = themes();
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("subset build + oracle");

    assert!(out.report.source_glyphs > 300, "fixture glyph count");
    assert!(
        out.report.total_glyphs < out.report.source_glyphs / 2,
        "glyph set collapses: {} of {}",
        out.report.total_glyphs,
        out.report.source_glyphs
    );
    // alt copies ride after the surviving source glyphs
    let kept_source = out.report.total_glyphs - out.report.alt_glyphs as u16;
    assert!(kept_source < out.report.source_glyphs / 2);
    assert!(out.report.verify.ok);

    // every char of the sample resolves in the produced font's cmap (and
    // unmapped chars — the \n — legitimately do not)
    use read_fonts::{FontRef, TableProvider};
    let font = FontRef::new(&out.ttf).expect("produced font parses");
    let charmap = skrifa::charmap::Charmap::new(&font);
    for ch in SAMPLE.chars() {
        if ch == '\n' {
            continue;
        }
        assert!(
            charmap.map(ch).is_some(),
            "sample char {ch:?} must survive subsetting"
        );
    }
    // the subset font keeps its outlines addressable: loca covers every
    // glyph id the cmap hands out
    let maxp = font.maxp().unwrap();
    let head = font.head().unwrap();
    let loca = font.loca(head.index_to_loc_format() == 1).unwrap();
    for gid in 0..maxp.num_glyphs() {
        assert!(loca.get_raw(gid as usize).is_some(), "loca covers {gid}");
    }
}

/// 0.2.3 regression, exact mode: the composite walker followed the wrong
/// flag bit layout (MORE_COMPONENTS 0x0010 instead of 0x0020), so
/// multi-component composites were truncated at the first component —
/// the closure dropped real components and the subset rewrote only the
/// first gid, leaving later ones pointing past the subset's glyph count.
/// Browsers refused the whole font at OTS ("Invalid glyph id used in
/// composite glyph") while the rustybuzz oracle stayed green (it never
/// walks outlines). This test walks the produced font's composites with
/// the implementation-standard bits and requires every component gid to
/// stay inside the subset.
#[test]
fn subset_composites_stay_in_range() {
    let themes = themes();
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("subset build");

    use read_fonts::{FontRef, TableProvider};
    let font = FontRef::new(&out.ttf).unwrap();
    let num = font.maxp().unwrap().num_glyphs() as usize;
    let head = font.head().unwrap();
    let loca = font.loca(head.index_to_loc_format() == 1).unwrap();
    let glyf = font
        .data_for_tag(write_fonts::types::Tag::new(b"glyf"))
        .unwrap()
        .as_bytes()
        .to_vec();

    let mut composites = 0;
    let mut multi = 0;
    for gid in 0..num {
        let (a, b) = (
            loca.get_raw(gid).unwrap() as usize,
            loca.get_raw(gid + 1).unwrap() as usize,
        );
        if b <= a {
            continue;
        }
        let g = &glyf[a..b];
        if g.len() < 12 || i16::from_be_bytes([g[0], g[1]]) >= 0 {
            continue;
        }
        composites += 1;
        let mut count = 0;
        let mut pos = 10;
        loop {
            assert!(
                pos + 4 <= g.len(),
                "glyph {gid}: composite runs past its bytes"
            );
            let flags = u16::from_be_bytes([g[pos], g[pos + 1]]);
            let comp = u16::from_be_bytes([g[pos + 2], g[pos + 3]]);
            assert!(
                (comp as usize) < num,
                "glyph {gid}: component {comp} leaves the subset ({num} glyphs) — browsers reject this at OTS"
            );
            count += 1;
            pos += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
            if flags & 0x0008 != 0 {
                pos += 2;
            } else if flags & 0x0040 != 0 {
                pos += 4;
            } else if flags & 0x0080 != 0 {
                pos += 8;
            }
            if flags & 0x0020 == 0 {
                break;
            }
        }
        if count > 1 {
            multi += 1;
        }
    }
    // the fixture carries 385 apple-layout composites; a real page sample
    // keeps at least one multi-component glyph, which is what the wrong
    // bits truncated
    assert!(composites > 0);
    assert!(
        multi > 0,
        "expected multi-component composites in the subset"
    );
}

/// 0.2.2 regression, generic mode: the non-subset glyf path
/// double-prefixed the loca offsets (an initial 0 plus the source loca's
/// own leading 0), shifting every source glyph one gid up. Composite
/// component gids inside the verbatim glyf bytes then pointed at the
/// wrong glyphs — including self-references — and browsers rejected the
/// whole font at OTS (fallback rendering, console spam), while the
/// rustybuzz oracle stayed green because it only diffs glyph ids and
/// never walks outlines. Lock the alignment and the composite graph.
#[test]
fn generic_font_glyphs_stay_aligned() {
    let themes = themes();
    let spec = chromaglyph_core::generic::rust();
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &[],
        run_verify: false,
        generic: Some(&spec),
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .expect("generic build");

    use read_fonts::TableProvider;
    use write_fonts::types::Tag;
    let src = read_fonts::FontRef::new(FONT).unwrap();
    let dst = read_fonts::FontRef::new(&out.ttf).unwrap();
    let num_src = src.maxp().unwrap().num_glyphs() as usize;
    let num_dst = dst.maxp().unwrap().num_glyphs() as usize;
    assert!(num_dst > num_src, "alt copies appended");

    let loca_of = |f: &read_fonts::FontRef<'_>| -> Vec<u32> {
        let head = f.head().unwrap();
        let loca = f.loca(head.index_to_loc_format() == 1).unwrap();
        (0..=f.maxp().unwrap().num_glyphs() as usize)
            .map(|i| loca.get_raw(i).expect("loca entry"))
            .collect()
    };
    let dloca = loca_of(&dst);
    assert_eq!(
        dloca.len(),
        num_dst + 1,
        "one offset per glyph plus the end"
    );
    assert_eq!(dloca[0], 0, "loca starts at 0 (no double prefix)");

    // every source-range glyph is byte-identical at the SAME gid
    let sloca = loca_of(&src);
    let sg: &[u8] = src.data_for_tag(Tag::new(b"glyf")).unwrap().as_bytes();
    let dg: &[u8] = dst.data_for_tag(Tag::new(b"glyf")).unwrap().as_bytes();
    for gid in 0..num_src {
        let (a, b) = (sloca[gid] as usize, sloca[gid + 1] as usize);
        let (c, d) = (dloca[gid] as usize, dloca[gid + 1] as usize);
        assert_eq!(
            &sg[a..b],
            &dg[c..d],
            "source glyph {gid} must stay at gid {gid} in the output"
        );
    }

    // no composite points outside the source range and none is cyclic
    fn components(glyf: &[u8], loca: &[u32], gid: usize) -> Vec<usize> {
        let (a, b) = (loca[gid] as usize, loca[gid + 1] as usize);
        if b <= a + 10 {
            return Vec::new();
        }
        let g = &glyf[a..b];
        if i16::from_be_bytes([g[0], g[1]]) >= 0 {
            return Vec::new();
        }
        let mut pos = 10;
        let mut comps = Vec::new();
        loop {
            let flags = u16::from_be_bytes([g[pos], g[pos + 1]]);
            let idx = u16::from_be_bytes([g[pos + 2], g[pos + 3]]) as usize;
            comps.push(idx);
            pos += 4;
            // args are 4 bytes with word args (0x0001), else 2; scale flags
            // carry 2/4/8 bytes (0x0008 / 0x0040 / 0x0080)
            pos += if flags & 0x0001 != 0 { 4 } else { 2 };
            if flags & 0x0008 != 0 {
                pos += 2;
            } else if flags & 0x0040 != 0 {
                pos += 4;
            } else if flags & 0x0080 != 0 {
                pos += 8;
            }
            if flags & 0x0020 == 0 {
                break; // 0x0020 = MORE_COMPONENTS
            }
        }
        comps
    }
    for gid in 0..num_src {
        let comps = components(dg, &dloca, gid);
        for c in &comps {
            assert!(
                *c < num_src,
                "glyph {gid} component {c} leaves the source range"
            );
        }
        // a component chain must never revisit its starting glyph
        let mut stack = comps.clone();
        let mut visited = std::collections::HashSet::new();
        while let Some(g) = stack.pop() {
            assert!(g != gid, "composite cycle revisits glyph {gid}");
            if visited.insert(g) {
                stack.extend(components(dg, &dloca, g));
            }
        }
    }
}

/// Exact mode with `ligatures: true`: the build shapes each block with the
/// source font (rustybuzz) and re-materializes every substitution as a
/// rule. This fixture expresses its `!=`/`->` "ligatures" as per-character
/// contextual alternates (same glyph count, different glyphs) — the
/// restore path must bring those alternates back, colored, with both the
/// build-time oracle and the standalone intent oracle green.
#[test]
fn exact_mode_restores_contextual_alternates() {
    let themes = themes();
    let src: &str = "fn f(x: u32) -> u32 { if x != 0 { x <= 2 } else { 3 } }\n";
    let out = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &[Block {
            id: "main".into(),
            lang: "rust".into(),
            text: src,
        }],
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: true,
        format: OutputFormat::Woff,
    })
    .expect("ligature build + oracle");

    assert!(
        out.report.lig_sites > 0,
        "the fixture ligates the operators"
    );
    assert!(out.report.lig_rules > 0);
    assert!(out.report.warnings.is_empty(), "{:?}", out.report.warnings);

    // the woff round-trips through the standalone intent oracle
    let result = verify(
        &out.woff,
        &[Block {
            id: "main".into(),
            lang: "rust".into(),
            text: src,
        }],
        &themes,
    )
    .expect("standalone verify");
    assert!(result.ok, "standalone verify: {:?}", result.failures);

    // shape the produced font: the `!` of `!=` must render its restored
    // alternate — neither the plain cmap glyph of '!' nor of '=' — and (the
    // operator token colors uniformly) that glyph must be a COLR colored copy
    use read_fonts::{FontRef, TableProvider};
    let font = FontRef::new(&out.ttf).unwrap();
    let charmap = skrifa::charmap::Charmap::new(&font);
    let bang = charmap.map('!').unwrap().to_u32() as u16;
    let eq = charmap.map('=').unwrap().to_u32() as u16;
    let face = rustybuzz::Face::from_slice(&out.ttf, 0).unwrap();
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(src);
    let shaped = rustybuzz::shape(&face, &[], buffer);
    let bang_at = shaped
        .glyph_infos()
        .iter()
        .find(|i| i.cluster == 18)
        .expect("a glyph covers the != site");
    let lig = bang_at.glyph_id as u16;
    assert_ne!(lig, bang, "the site is not the plain ! glyph");
    assert_ne!(lig, eq, "the site is not the plain = glyph");
    let colr = font.colr().unwrap();
    let bases: Vec<u16> = colr
        .base_glyph_records()
        .unwrap()
        .unwrap()
        .iter()
        .map(|r| r.glyph_id().to_u32() as u16)
        .collect();
    assert!(
        bases.contains(&lig),
        "the colored ligature copy paints the ligature glyph itself"
    );

    // contrast: the same text built WITHOUT ligatures keeps the two base
    // glyphs (no restore rules at all)
    let plain = build(&BuildInput {
        font_bytes: FONT,
        font_index: 0,
        themes: &themes,
        blocks: &[Block {
            id: "main".into(),
            lang: "rust".into(),
            text: src,
        }],
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: false,
        format: OutputFormat::Woff,
    })
    .unwrap();
    assert_eq!(plain.report.lig_sites, 0);
    assert_eq!(plain.report.lig_rules, 0);
    assert!(plain.report.verify.ok);
}

/// A real LigatureSubst, exercised through a fixture splice: the JBM
/// fixture only substitutes per-char alternates, so this builds a font
/// whose `calt` carries one honest ligature (`f` + `i` → the glyph of
/// `W`). All three fates get covered: colored (inside a string, uniform
/// slot), uncolored (a plain identifier), and mixed (an identifier with a
/// semantic overlay on the second char) which must degrade so colors win.
#[test]
fn injected_ligature_colored_uncolored_and_mixed() {
    let themes = themes();
    let lig_font = inject_gsub_ligature(FONT, 'f', 'i', fixture_gid('W'));

    // colored: "fi" inside a string colors uniformly, so the ligature
    // glyph itself gets the colored copy
    let colored_src: &str = "let s = \"fi\";\n";
    let out = build(&BuildInput {
        font_bytes: &lig_font,
        font_index: 0,
        themes: &themes,
        blocks: &[Block {
            id: "main".into(),
            lang: "rust".into(),
            text: colored_src,
        }],
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: true,
        format: OutputFormat::Woff,
    })
    .expect("colored ligature build + oracle");
    assert!(out.report.warnings.is_empty(), "{:?}", out.report.warnings);
    use read_fonts::{FontRef, TableProvider};
    let font = FontRef::new(&out.ttf).unwrap();
    let charmap = skrifa::charmap::Charmap::new(&font);
    let f_gid = charmap.map('f').unwrap().to_u32() as u16;
    let i_gid = charmap.map('i').unwrap().to_u32() as u16;
    let face = rustybuzz::Face::from_slice(&out.ttf, 0).unwrap();
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(colored_src);
    let shaped = rustybuzz::shape(&face, &[], buffer);
    let site = colored_src.find("fi").unwrap();
    // ONE glyph at the site cluster: not f, not i — the ligature
    let at_site: Vec<u16> = shaped
        .glyph_infos()
        .iter()
        .filter(|g| g.cluster as usize == site)
        .map(|g| g.glyph_id as u16)
        .collect();
    assert_eq!(at_site.len(), 1, "the site renders as one glyph");
    let lig = at_site[0];
    assert_ne!(lig, f_gid);
    assert_ne!(lig, i_gid);
    // and it is a COLR colored copy (the string colors uniformly)
    let colr = font.colr().unwrap();
    let bases: Vec<u16> = colr
        .base_glyph_records()
        .unwrap()
        .unwrap()
        .iter()
        .map(|r| r.glyph_id().to_u32() as u16)
        .collect();
    assert!(bases.contains(&lig), "the ligature glyph is colored");
    // the standalone intent oracle round-trips through the woff
    let result = verify(
        &out.woff,
        &[Block {
            id: "main".into(),
            lang: "rust".into(),
            text: colored_src,
        }],
        &themes,
    )
    .unwrap();
    assert!(result.ok, "{:?}", result.failures);

    // uncolored: the same ligature over a plain identifier restores the
    // ligature glyph WITHOUT a colored copy
    let plain_src: &str = "let fi = 1;\n";
    let out = build(&BuildInput {
        font_bytes: &lig_font,
        font_index: 0,
        themes: &themes,
        blocks: &[Block {
            id: "main".into(),
            lang: "rust".into(),
            text: plain_src,
        }],
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: None,
        ligatures: true,
        format: OutputFormat::Woff,
    })
    .expect("uncolored ligature build + oracle");
    let font = FontRef::new(&out.ttf).unwrap();
    let face = rustybuzz::Face::from_slice(&out.ttf, 0).unwrap();
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(plain_src);
    let shaped = rustybuzz::shape(&face, &[], buffer);
    let site = plain_src.find("fi").unwrap();
    let at_site: Vec<u16> = shaped
        .glyph_infos()
        .iter()
        .filter(|g| g.cluster as usize == site)
        .map(|g| g.glyph_id as u16)
        .collect();
    assert_eq!(at_site.len(), 1, "uncolored site still ligates");
    let is_colored = font
        .colr()
        .unwrap()
        .base_glyph_records()
        .unwrap()
        .unwrap()
        .iter()
        .any(|r| r.glyph_id().to_u32() as u16 == at_site[0]);
    assert!(!is_colored, "the uncolored ligature has no colored copy");

    // mixed: a semantic overlay colors only the `i` — the ligature gives
    // way so every character keeps its color, with a warning
    let mixed_src: &str = "let fi = 1;\n";
    let fi_at = mixed_src.find("fi").unwrap();
    let overlay = move |block: &chromaglyph_core::Block<'_>| -> Vec<(usize, usize, String)> {
        let _ = block;
        vec![(fi_at + 1, fi_at + 2, "variable.parameter".to_string())]
    };
    let out = build(&BuildInput {
        font_bytes: &lig_font,
        font_index: 0,
        themes: &themes,
        blocks: &[Block {
            id: "main".into(),
            lang: "rust".into(),
            text: mixed_src,
        }],
        run_verify: true,
        generic: None,
        font_name: None,
        semantic: Some(&overlay),
        ligatures: true,
        format: OutputFormat::Woff,
    })
    .expect("mixed ligature build + oracle");
    assert!(
        out.report
            .warnings
            .iter()
            .any(|w| w.contains("ligature") && w.contains("color")),
        "{:?}",
        out.report.warnings
    );
    let face = rustybuzz::Face::from_slice(&out.ttf, 0).unwrap();
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(mixed_src);
    let shaped = rustybuzz::shape(&face, &[], buffer);
    // per-build subset numbering: read the mixed font's own cmap
    let mixed_font = FontRef::new(&out.ttf).unwrap();
    let mixed_charmap = skrifa::charmap::Charmap::new(&mixed_font);
    let mixed_f = mixed_charmap.map('f').unwrap().to_u32() as u16;
    let at_site: Vec<u16> = shaped
        .glyph_infos()
        .iter()
        .filter(|g| g.cluster as usize == fi_at)
        .map(|g| g.glyph_id as u16)
        .collect();
    assert_eq!(at_site, vec![mixed_f], "the mixed site stays per-char");
    // and the second char still carries its overlay color: the i glyph is
    // a colored copy
    let mixed_i = mixed_charmap.map('i').unwrap().to_u32() as u16;
    let i_at = shaped
        .glyph_infos()
        .iter()
        .find(|g| g.cluster as usize == fi_at + 1)
        .unwrap();
    assert_ne!(i_at.glyph_id as u16, mixed_i, "the overlaid char colors");
}

/// Gid of `ch` in the untouched fixture (same numbering as any build's
/// source font).
fn fixture_gid(ch: char) -> u16 {
    let font = read_fonts::FontRef::new(FONT).unwrap();
    skrifa::charmap::Charmap::new(&font)
        .map(ch)
        .map(|g| g.to_u32() as u16)
        .unwrap_or(0)
}

/// Splice a `calt` GSUB carrying one LigatureSubst (`first` + `second` →
/// `out_gid`) into `font`, replacing its own layout tables — the JBM
/// fixture only substitutes per-char alternates, so true ligatures need a
/// dedicated fixture.
fn inject_gsub_ligature(font: &[u8], first: char, second: char, out_gid: u16) -> Vec<u8> {
    use read_fonts::{FontRef, TableProvider};
    use write_fonts::tables::gsub::{
        Gsub, Ligature, LigatureSet, LigatureSubstFormat1, SubstitutionLookup,
    };
    use write_fonts::tables::layout::builders::CoverageTableBuilder;
    use write_fonts::tables::layout::{
        Feature, FeatureList, FeatureRecord, LangSys, Lookup, LookupFlag, LookupList, Script,
        ScriptList, ScriptRecord,
    };
    use write_fonts::types::{GlyphId16, Tag};

    let parsed = FontRef::new(font).expect("fixture parses");
    let charmap = skrifa::charmap::Charmap::new(&parsed);
    let g1 = charmap.map(first).expect("first maps").to_u32() as u16;
    let g2 = charmap.map(second).expect("second maps").to_u32() as u16;

    let mut coverage = CoverageTableBuilder::default();
    coverage.add(GlyphId16::new(g1));
    let ligature = Ligature::new(GlyphId16::new(out_gid), vec![GlyphId16::new(g2)]);
    let lookup = SubstitutionLookup::Ligature(Lookup::new(
        LookupFlag::empty(),
        vec![LigatureSubstFormat1::new(
            coverage.build(),
            vec![LigatureSet::new(vec![ligature])],
        )],
    ));
    let feature_list = FeatureList::new(vec![FeatureRecord::new(
        Tag::new(b"calt"),
        Feature::new(None, vec![0]),
    )]);
    let lang_sys = LangSys::new(vec![0]);
    let script = Script::new(Some(lang_sys.clone()), Vec::new());
    let script_list = ScriptList::new(vec![
        ScriptRecord::new(Tag::new(b"DFLT"), script.clone()),
        ScriptRecord::new(Tag::new(b"latn"), script),
    ]);
    let gsub = Gsub::new(script_list, feature_list, LookupList::new(vec![lookup]));

    let mut builder = write_fonts::FontBuilder::new();
    for record in parsed.table_directory().table_records() {
        let tag = record.tag();
        if tag == Tag::new(b"GSUB") || tag == Tag::new(b"GPOS") || tag == Tag::new(b"GDEF") {
            continue;
        }
        builder.add_raw(tag, parsed.data_for_tag(tag).unwrap().as_bytes().to_vec());
    }
    builder.add_table(&gsub).unwrap();
    builder.build()
}
