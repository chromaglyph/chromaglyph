//! End-to-end: fixture TTF + code snippet + two themes → one color font
//! with two CPAL palettes, verified by the rustybuzz oracle (inside
//! `build`), then re-verified standalone from the woff bytes.

use chromaglyph_core::theme::ThemeSpec;
use chromaglyph_core::{build, verify, Block, BuildInput};

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
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
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
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: true,
    })
    .expect("single-theme build");
    assert_eq!(out.report.palettes.len(), 1);
    assert!(out.report.verify.ok);
}

#[test]
fn rejects_cff_fonts() {
    let themes = themes();
    // the reference project's CFF-flavored otf
    let path = "/Users/taiyuuki/projects/i/font/FontWithASyntaxHighlighter/FontWithASyntaxHighlighter-Regular.otf";
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("skipping: reference otf not present");
        return;
    };
    let err = match build(&BuildInput {
        font_bytes: &bytes,
        themes: &themes,
        blocks: &blocks("fn"),
        run_verify: false,
    }) {
        Ok(_) => panic!("CFF font must be rejected"),
        Err(err) => err,
    };
    assert!(
        matches!(err, chromaglyph_core::BuildError::NotTrueType),
        "got: {err}"
    );
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
        themes: &themes,
        blocks: &blocks,
        run_verify: true,
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
        themes: &themes,
        blocks: &blocks(SAMPLE),
        run_verify: false,
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
        themes: &themes,
        blocks: &blocks(EMOJI_SAMPLE),
        run_verify: true,
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
        themes: &themes,
        blocks: &blocks(UVS_SAMPLE),
        run_verify: true,
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
