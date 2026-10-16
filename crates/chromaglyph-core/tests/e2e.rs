//! M1 end-to-end: fixture TTF + code snippet → colored font, verified by
//! the rustybuzz oracle (run inside `build`), then re-verified standalone
//! from the woff bytes.

use chromaglyph_core::{build, verify, Block, BuildInput, Theme};

const FONT: &[u8] = include_bytes!("fixtures/JetBrainsMono-Regular.ttf");

const SAMPLE: &str = r#"// chromaglyph m1
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

#[test]
fn m1_end_to_end() {
    let theme = Theme::builtin_dark();
    let out = build(&BuildInput {
        font_bytes: FONT,
        theme: &theme,
        blocks: &blocks(SAMPLE),
        run_verify: true,
    })
    .expect("build + inline verify must succeed");

    assert!(out.report.rules > 0, "expected some substitution rules");
    assert!(out.report.alt_glyphs > 0);
    assert_eq!(
        out.report.palette.len(),
        5,
        "keyword/string/comment/number/identifier colors"
    );
    assert_eq!(
        out.report.source_glyphs + out.report.alt_glyphs as u16,
        out.report.total_glyphs
    );
    assert!(out.report.verify.ok);
    assert_eq!(out.report.verify.failures.len(), 0);
    assert!(out.report.verify.colored_glyphs > 0);
    // the exact-mode GSUB stays tiny even though the source font's layout
    // tables were dropped (GSUB+GPOS+GDEF of JetBrains Mono are large)
    assert!(
        out.report.tables["GSUB"] < 8 * 1024,
        "GSUB should be small: {:?}",
        out.report.tables["GSUB"]
    );
    assert!(out.woff.len() < out.ttf.len(), "woff must compress");

    // tables of interest exist
    for table in ["GSUB", "COLR", "CPAL", "glyf", "loca", "hmtx"] {
        assert!(
            out.report.tables.contains_key(table),
            "missing table {table}: {:?}",
            out.report.tables.keys().collect::<Vec<_>>()
        );
    }

    // the woff round-trips: unwrap it and run the standalone oracle
    let ttf_again = chromaglyph_core::woff::unwrap(&out.woff).expect("woff must unwrap");
    let result = verify(&out.woff, &blocks(SAMPLE), &theme).expect("standalone verify");
    assert!(
        result.ok,
        "standalone verify from woff: {:?}",
        result.failures
    );
    let _ = ttf_again;
}

#[test]
fn rejects_cff_fonts() {
    let theme = Theme::builtin_dark();
    // the reference project's CFF-flavored otf
    let path = "/Users/taiyuuki/projects/i/font/FontWithASyntaxHighlighter/FontWithASyntaxHighlighter-Regular.otf";
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("skipping: reference otf not present");
        return;
    };
    let err = match build(&BuildInput {
        font_bytes: &bytes,
        theme: &theme,
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
fn ambiguity_across_blocks_degrades_not_fails() {
    let theme = Theme::builtin_dark();
    let blocks = vec![
        Block {
            id: "rust-block".into(),
            lang: "rust".into(),
            text: "let",
        },
        Block {
            id: "js-block".into(),
            lang: "plain".into(),
            text: "let",
        },
    ];
    // identical block text, different language colors: any rule would fire
    // in both blocks, so the build degrades both to uncolored (with a
    // warning) instead of failing
    let out = build(&BuildInput {
        font_bytes: FONT,
        theme: &theme,
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
