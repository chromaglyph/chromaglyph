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
