//! # chromaglyph-core
//!
//! Compile syntax highlighting into an OpenType font.
//!
//! Given a static TrueType font, a set of themes and code blocks whose
//! content is known at build time ("exact mode"), produce a color font
//! (COLRv0 + one CPAL palette per theme) whose `calt` substitution rules
//! color exactly those tokens. The text in the DOM stays byte-identical:
//! highlighting happens entirely inside the font's glyph substitutions, and
//! theme switching is a pure `@font-palette-values` flip.
//!
//! Pipeline:
//! 1. [`tokenize`] — syntect grammar → per-character color under each theme
//! 2. [`compile`] — colors → chain-context rules with minimal distinguishing
//!    contexts, audited globally against false matches (slot model: one rule
//!    set serves every theme)
//! 3. [`fontbuild`] — assemble the output font (empty colored copies + COLR
//!    layers referencing base glyphs)
//! 4. [`verify`] — rustybuzz oracle + per-palette color readback

#![forbid(unsafe_code)]

pub mod compile;
pub mod error;
pub mod fontbuild;
pub mod gsub;
pub mod spec;
pub mod theme;
pub mod tokenize;
pub mod verify;
pub mod woff;

pub use error::BuildError;
pub use spec::{Block, Color};
pub use theme::ThemeSpec;
pub use verify::VerifyResult;

use std::collections::BTreeMap;

use compile::PreparedBlock;
use syntect::parsing::SyntaxSet;

/// Build request.
pub struct BuildInput<'a> {
    /// Static TrueType font bytes (TTF; CFF and variable fonts rejected).
    pub font_bytes: &'a [u8],
    /// One or more themes; palette `i` in the output font renders theme `i`.
    /// The first theme is the default palette.
    pub themes: &'a [ThemeSpec],
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
    pub themes: Vec<String>,
    pub blocks: usize,
    pub chars: usize,
    pub colored_cells: usize,
    pub slots: usize,
    pub rules: usize,
    pub alt_glyphs: usize,
    pub source_glyphs: u16,
    pub total_glyphs: u16,
    /// Per theme: the hex color of every slot (palette contents).
    pub palettes: Vec<Vec<String>>,
    pub tables: BTreeMap<String, usize>,
    pub ttf_bytes: usize,
    pub woff_bytes: usize,
    pub warnings: Vec<String>,
    pub verify: VerifyResult,
}

pub fn build(input: &BuildInput<'_>) -> Result<BuildOutput, BuildError> {
    if input.themes.is_empty() {
        return Err(BuildError::Theme("at least one theme is required".into()));
    }
    let loaded = fontbuild::load(input.font_bytes)?;

    // stage 1+2: tokenize + resolve colors under each theme
    let ss = SyntaxSet::load_defaults_newlines();
    let prepared: Vec<PreparedBlock<'_>> = input
        .blocks
        .iter()
        .map(|block| {
            Ok(PreparedBlock {
                id: block.id.clone(),
                text: block.text,
                colors: tokenize::colorize_block(block, input.themes, &ss)?,
            })
        })
        .collect::<Result<_, BuildError>>()?;
    let theme_fgs: Vec<Color> = input.themes.iter().map(|t| t.foreground).collect();

    // stage 3: compile to rules (slot model)
    let charmap = skrifa::charmap::Charmap::new(&loaded.font);
    let plan = compile::compile(
        &prepared,
        &|ch, vs| {
            // base + variation selector: resolve through cmap14 when the
            // font carries a non-default UVS mapping for the sequence
            if let Some(vs) = vs {
                if let Some(skrifa::charmap::MapVariant::Variant(g)) =
                    charmap.map_variant(ch, vs)
                {
                    return Some(g.to_u32() as u16).filter(|g| *g != 0);
                }
            }
            charmap
                .map(ch)
                .map(|g| g.to_u32() as u16)
                .filter(|g| *g != 0)
        },
        loaded.num_glyphs,
        &theme_fgs,
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
    let css = render_css(input.themes);

    let report = Report {
        themes: input.themes.iter().map(|t| t.name.clone()).collect(),
        blocks: input.blocks.len(),
        chars: plan.cells.len(),
        colored_cells: plan.cells.iter().filter(|c| c.pal.is_some()).count(),
        slots: plan.slots.len(),
        rules: plan.rules.len(),
        alt_glyphs: plan.alt_glyphs.len(),
        source_glyphs: loaded.num_glyphs,
        total_glyphs: loaded.num_glyphs + plan.alt_glyphs.len() as u16,
        palettes: (0..input.themes.len())
            .map(|t| {
                plan.slots
                    .iter()
                    .map(|vector| vector[t].unwrap_or(theme_fgs[t]).to_hex())
                    .collect()
            })
            .collect(),
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

/// Verify a produced font (.ttf or .woff) against input blocks + themes.
pub fn verify(
    font_bytes: &[u8],
    blocks: &[Block<'_>],
    themes: &[ThemeSpec],
) -> Result<VerifyResult, BuildError> {
    let ttf = if font_bytes.len() >= 4 && &font_bytes[0..4] == b"wOFF" {
        woff::unwrap(font_bytes)?
    } else {
        font_bytes.to_vec()
    };
    verify::verify_font(&ttf, blocks, themes)
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

/// CSS companion: @font-face, one @font-palette-values per theme, and (when
/// a light theme is present) automatic light/dark switching via
/// `prefers-color-scheme`. Editor background colors go to CSS, never into
/// the font.
fn render_css(themes: &[ThemeSpec]) -> String {
    let mut css = String::from("/* generated by chromaglyph */\n");
    css.push_str("@font-face {\n  font-family: 'chromaglyph';\n  src: url('chromaglyph.woff') format('woff');\n}\n\n");
    let slug = |name: &str| -> String {
        name.to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_string()
    };
    for (i, theme) in themes.iter().enumerate() {
        css.push_str(&format!(
            "@font-palette-values --chromaglyph-{} {{\n  font-family: chromaglyph;\n  base-palette: {i};\n}}\n\n",
            slug(&theme.name)
        ));
    }
    css.push_str("pre, code {\n  font-family: chromaglyph, monospace;\n");
    if let Some(bg) = themes.first().and_then(|t| t.background) {
        css.push_str(&format!("  background: {};\n", bg.to_hex()));
    }
    css.push_str("}\n");
    // second theme switches automatically with the OS color scheme
    if themes.len() >= 2 {
        let light = &themes[1];
        css.push_str(&format!(
            "\n@media (prefers-color-scheme: {}) {{\n  pre, code {{\n    font-palette: --chromaglyph-{};\n",
            "light",
            slug(&light.name)
        ));
        if let Some(bg) = light.background {
            css.push_str(&format!("    background: {};\n", bg.to_hex()));
        }
        css.push_str("  }\n}\n");
    }
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_hex_roundtrip() {
        let c = Color::from_hex("#569cd6").unwrap();
        assert_eq!(c.to_hex(), "#569cd6");
        assert_eq!(Color::from_hex("#569cd6ff").unwrap().alpha, 255);
        assert!(Color::from_hex("#569cd").is_none());
        assert!(Color::from_hex("569cd6").is_none());
    }

    #[test]
    fn css_contains_palette_values() {
        let themes = vec![
            ThemeSpec::builtin("dark-plus").unwrap(),
            ThemeSpec::builtin("light-plus").unwrap(),
        ];
        let css = render_css(&themes);
        assert!(css.contains("base-palette: 0"));
        assert!(css.contains("base-palette: 1"));
        assert!(css.contains("prefers-color-scheme: light"));
        assert!(css.contains("#1e1e1e")); // dark bg from the theme
    }
}
