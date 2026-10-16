//! # chromaglyph-core
//!
//! Compile syntax highlighting into an OpenType font.
//!
//! Given a static TrueType font, a theme and code blocks whose content is
//! known at build time ("exact mode"), produce a color font (COLRv0 + CPAL)
//! whose `calt` substitution rules color exactly those tokens. The text in
//! the DOM stays byte-identical: highlighting happens entirely inside the
//! font's glyph substitutions.
//!
//! Pipeline:
//! 1. `tokenize` — (lang, text) → tokens (M1: hardcoded lexer; M2: syntect)
//! 2. `theme` — scope → palette index
//! 3. [`compile`] — occurrences → chain-context rules with minimal
//!    distinguishing contexts, audited globally against false matches
//! 4. [`fontbuild`] — assemble the output font (empty colored copies + COLR
//!    layers referencing base glyphs)
//! 5. [`verify`] — rustybuzz oracle + COLR/CPAL color readback

#![forbid(unsafe_code)]

pub mod compile;
pub mod error;
pub mod fontbuild;
pub mod gsub;
pub mod spec;
pub mod tokenize;
pub mod verify;
pub mod woff;

pub use error::BuildError;
pub use spec::{Block, Color, Theme, Token};
pub use verify::VerifyResult;

use std::collections::BTreeMap;

use compile::PreparedBlock;

/// Build request.
pub struct BuildInput<'a> {
    /// Static TrueType font bytes (TTF; CFF and variable fonts rejected).
    pub font_bytes: &'a [u8],
    pub theme: &'a Theme,
    pub blocks: &'a [Block<'a>],
    /// Run the rustybuzz oracle and fail the build on mismatch (default on).
    pub run_verify: bool,
}

/// Build artifacts.
pub struct BuildOutput {
    /// The assembled sfnt (raw TrueType).
    pub ttf: Vec<u8>,
    /// WOFF1 (zlib) wrapping of `ttf`.
    pub woff: Vec<u8>,
    pub css: String,
    pub report: Report,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Report {
    pub blocks: usize,
    pub chars: usize,
    pub tokens: usize,
    pub uncolored_tokens: usize,
    pub rules: usize,
    pub alt_glyphs: usize,
    pub source_glyphs: u16,
    pub total_glyphs: u16,
    pub palette: Vec<String>,
    pub tables: BTreeMap<String, usize>,
    pub ttf_bytes: usize,
    pub woff_bytes: usize,
    pub warnings: Vec<String>,
    pub verify: VerifyResult,
}

pub fn build(input: &BuildInput<'_>) -> Result<BuildOutput, BuildError> {
    let loaded = fontbuild::load(input.font_bytes)?;

    // stage 1+2: tokenize blocks
    let prepared: Vec<PreparedBlock<'_>> = input
        .blocks
        .iter()
        .map(|block| PreparedBlock {
            id: block.id.clone(),
            text: block.text,
            tokens: tokenize::tokenize(block.text, &block.lang),
        })
        .collect();

    // stage 3: compile to rules
    let charmap = skrifa::charmap::Charmap::new(&loaded.font);
    let plan = compile::compile(
        &prepared,
        input.theme,
        &|ch| {
            charmap
                .map(ch)
                .map(|g| g.to_u32() as u16)
                .filter(|g| *g != 0)
        },
        loaded.num_glyphs,
    )?;

    // stage 4: assemble font
    let gsub = gsub::build_gsub(&plan);
    let ttf = fontbuild::assemble(&loaded, &plan, gsub.as_ref())?;

    // stage 5: verify (default on)
    let verify_result = if input.run_verify {
        let result = verify::verify_plan(&ttf, &prepared, &plan);
        if !result.ok {
            return Err(BuildError::VerifyFailed(result.failures));
        }
        result
    } else {
        let mut result = VerifyResult::ok();
        result.glyphs_checked = plan.cells.len();
        result.colored_glyphs = plan.cells.iter().filter(|c| c.pal.is_some()).count();
        result
    };

    let woff = woff::wrap(&ttf)?;
    let css = render_css(input.theme);

    let report = Report {
        blocks: input.blocks.len(),
        chars: plan.cells.len(),
        tokens: plan.total_tokens,
        uncolored_tokens: plan.uncolored_tokens,
        rules: plan.rules.len(),
        alt_glyphs: plan.alt_glyphs.len(),
        source_glyphs: loaded.num_glyphs,
        total_glyphs: loaded.num_glyphs + plan.alt_glyphs.len() as u16,
        palette: plan.palette.iter().map(|c| c.to_hex()).collect(),
        tables: table_sizes(&ttf),
        ttf_bytes: ttf.len(),
        woff_bytes: woff.len(),
        warnings: plan.warnings,
        verify: verify_result,
    };

    Ok(BuildOutput {
        ttf,
        woff,
        css,
        report,
    })
}

/// Verify a produced font (.ttf or .woff) against input blocks + theme.
pub fn verify(
    font_bytes: &[u8],
    blocks: &[Block<'_>],
    theme: &Theme,
) -> Result<VerifyResult, BuildError> {
    let ttf = if font_bytes.len() >= 4 && &font_bytes[0..4] == b"wOFF" {
        woff::unwrap(font_bytes)?
    } else {
        font_bytes.to_vec()
    };
    let prepared: Vec<PreparedBlock<'_>> = blocks
        .iter()
        .map(|block| PreparedBlock {
            id: block.id.clone(),
            text: block.text,
            tokens: tokenize::tokenize(block.text, &block.lang),
        })
        .collect();
    verify::verify_font(&ttf, &prepared, theme)
}

fn table_sizes(ttf: &[u8]) -> BTreeMap<String, usize> {
    let mut sizes = BTreeMap::new();
    if let Ok(font) = read_fonts::FontRef::new(ttf) {
        for record in font.table_directory().table_records() {
            sizes.insert(record.tag().to_string(), record.length() as usize);
        }
    }
    sizes
}

/// Minimal CSS companion for M1; M2 adds @font-palette-values for light/dark
/// switching across multiple theme palettes.
fn render_css(theme: &Theme) -> String {
    format!(
        "/* generated by chromaglyph (theme: {}) */\n\
         @font-face {{\n  font-family: 'chromaglyph';\n  src: url('chromaglyph.woff') format('woff');\n}}\n\n\
         /* stack the highlighted font first, fall back to the base font */\n\
         pre, code {{ font-family: chromaglyph, monospace; }}\n",
        theme.name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_hex_roundtrip() {
        let c = Color::from_hex6("#569cd6").unwrap();
        assert_eq!(c.to_hex(), "#569cd6");
        assert!(Color::from_hex6("#569cd").is_none());
        assert!(Color::from_hex6("569cd6").is_none());
    }
}
