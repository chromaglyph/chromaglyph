//! Stage 1+2 of the pipeline: tokenize with real grammars (syntect, the bat
//! engine, pure-Rust fancy-regex build) and resolve each character's color
//! under each theme.
//!
//! What comes out is concrete colors, not scope names: syntect's selector
//! engine does the scope→style resolution per theme, and a color equal to
//! the theme's default foreground is reported as "uncolored" (the char keeps
//! its base glyph and follows the page's CSS foreground, exactly like an
//! editor renders unstyled text).

use syntect::easy::HighlightLines;
use syntect::parsing::SyntaxSet;

use crate::error::BuildError;
use crate::spec::{Block, Color};
use crate::theme::ThemeSpec;

/// Common language-name aliases → file extensions understood by syntect.
const LANG_ALIASES: &[(&str, &str)] = &[
    ("rust", "rs"),
    ("javascript", "js"),
    ("typescript", "ts"),
    ("python", "py"),
    ("go", "go"),
    ("c", "c"),
    ("cpp", "cpp"),
    ("c++", "cpp"),
    ("csharp", "cs"),
    ("java", "java"),
    ("json", "json"),
    ("yaml", "yaml"),
    ("toml", "toml"),
    ("html", "html"),
    ("css", "css"),
    ("shell", "sh"),
    ("bash", "sh"),
    ("sql", "sql"),
    ("markdown", "md"),
    ("plain", "txt"),
];

/// The syntax set: syntect's defaults plus the extras two-face bundles
/// (TypeScript/TSX among them — the defaults alone cannot resolve `ts`,
/// which silently fell back to plain text before). Built once per process.
pub fn syntax_set() -> &'static syntect::parsing::SyntaxSet {
    static SS: std::sync::OnceLock<syntect::parsing::SyntaxSet> = std::sync::OnceLock::new();
    SS.get_or_init(two_face::syntax::extra_newlines)
}

/// Resolve a `--lang` value (alias, extension, or "plain") to a syntect
/// syntax; unknown values fall back to Plain Text with an error-free life.
pub fn resolve_syntax<'a>(lang: &str, ss: &'a SyntaxSet) -> &'a syntect::parsing::SyntaxReference {
    let key = lang.trim().to_ascii_lowercase();
    if key == "plain" || key == "txt" || key.is_empty() {
        return ss.find_syntax_plain_text();
    }
    let ext = LANG_ALIASES
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, ext)| *ext)
        .unwrap_or(&key);
    ss.find_syntax_by_extension(ext)
        .unwrap_or_else(|| ss.find_syntax_plain_text())
}

/// Resolve per-character colors for one block under every theme.
///
/// Returns `colors[theme][char]`: `Some(color)` when the theme colors this
/// char differently from its default foreground, `None` otherwise.
pub fn colorize_block(
    block: &Block<'_>,
    themes: &[ThemeSpec],
    ss: &SyntaxSet,
) -> Result<Vec<Vec<Option<Color>>>, BuildError> {
    let syntax = resolve_syntax(&block.lang, ss);
    let mut by_theme = Vec::with_capacity(themes.len());
    for theme in themes {
        let mut hl = HighlightLines::new(syntax, &theme.theme);
        let default_fg = syntect_color(theme.foreground);
        let mut colors = Vec::new();
        for line in block.text.split_inclusive('\n') {
            let regions = hl
                .highlight_line(line, ss)
                .map_err(|e| BuildError::Tokenize(format!("block '{}': {e}", block.id)))?;
            for (style, region) in regions {
                let colored = if style.foreground == default_fg {
                    None
                } else {
                    Some(Color::rgb(
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                    ))
                };
                // one entry per char (regions are char-aligned substrings)
                colors.extend(std::iter::repeat_n(colored, region.chars().count()));
            }
        }
        let char_count = block.text.chars().count();
        if colors.len() != char_count {
            return Err(BuildError::Tokenize(format!(
                "block '{}': highlighted {} chars but text has {}",
                block.id,
                colors.len(),
                char_count
            )));
        }
        by_theme.push(colors);
    }
    Ok(by_theme)
}

fn syntect_color(c: Color) -> syntect::highlighting::Color {
    syntect::highlighting::Color {
        r: c.red,
        g: c.green,
        b: c.blue,
        a: c.alpha,
    }
}

/// Overlay a semantic layer onto the syntactic colors: each span is a
/// `(byte_start, byte_end, scope)` triple; where a theme colors the scope,
/// every char of the span takes that color (semantic wins over syntax).
/// Scopes no theme colors are ignored, so themes without semantic rules
/// are unaffected.
pub fn apply_semantic(
    colors: &mut [Vec<Option<Color>>],
    text: &str,
    spans: &[(usize, usize, String)],
    themes: &[ThemeSpec],
) {
    if spans.is_empty() {
        return;
    }
    // byte offset -> char index (spans align to node boundaries = char starts)
    let char_at: std::collections::HashMap<usize, usize> = text
        .char_indices()
        .enumerate()
        .map(|(ci, (byte, _))| (byte, ci))
        .collect();
    let total_chars = text.chars().count();
    for (start, end, scope) in spans {
        let resolved: Vec<Option<Color>> = themes.iter().map(|t| t.scope_color(scope)).collect();
        if resolved.iter().all(|c| c.is_none()) {
            continue; // no theme knows this scope: keep syntactic colors
        }
        let Some(&cs) = char_at.get(start) else {
            continue;
        };
        let ce = char_at.get(end).copied().unwrap_or(total_chars);
        if ce <= cs {
            continue;
        }
        for (row, color) in colors.iter_mut().zip(resolved) {
            if let Some(color) = color {
                row[cs..ce].fill(Some(color));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn themes() -> Vec<ThemeSpec> {
        vec![ThemeSpec::builtin("dark-plus").unwrap()]
    }

    #[test]
    fn rust_colors_are_resolved() {
        let ss = crate::tokenize::syntax_set().clone();
        let text = "fn main() { let s = \"hi\"; } // done\n";
        let block = Block {
            id: "t".into(),
            lang: "rust".into(),
            text,
        };
        let colors = colorize_block(&block, &themes(), &ss).unwrap();
        let chars: Vec<char> = text.chars().collect();
        // `fn` (chars 0,1) is keyword blue in Dark+
        assert_eq!(colors[0][0].map(|c| c.to_hex()), Some("#569cd6".into()));
        // space (char 2) is default foreground -> uncolored
        assert_eq!(colors[0][2], None);
        // `s` inside "hi" string region colored, quotes too
        let quote_open = chars.iter().position(|c| *c == '"').unwrap();
        assert!(colors[0][quote_open].is_some());
        // comment green
        let comment = text.find("//").unwrap();
        let idx = text[..comment].chars().count();
        assert_eq!(colors[0][idx].map(|c| c.to_hex()), Some("#6a9955".into()));
    }

    #[test]
    fn char_count_matches_for_multibyte() {
        let ss = crate::tokenize::syntax_set().clone();
        let text = "// 注释 émoji 🎨 ok\nlet x = \"héllo\";\n";
        let block = Block {
            id: "t".into(),
            lang: "rust".into(),
            text,
        };
        let colors = colorize_block(&block, &themes(), &ss).unwrap();
        assert_eq!(colors[0].len(), text.chars().count());
    }

    #[test]
    fn unknown_lang_falls_back_to_plain() {
        let ss = crate::tokenize::syntax_set().clone();
        let block = Block {
            id: "t".into(),
            lang: "definitely-not-a-lang".into(),
            text: "let x",
        };
        let colors = colorize_block(&block, &themes(), &ss).unwrap();
        assert!(
            colors[0].iter().all(|c| c.is_none()),
            "plain text has no coloring"
        );
    }

    #[test]
    fn semantic_object_key_colors_js_like_ts() {
        // the JS grammar scopes object keys `meta.mapping.key` where TS's
        // uses `meta.object-literal.key` — dark-plus colors only the
        // latter, so a JS key stays plain syntactically; the semantic
        // layer's property span must resolve to the same blue or JS never
        // matches TS in an editor
        let ss = crate::tokenize::syntax_set();
        let th = themes();
        let js = "const o = {\n  msg: 'message'\n};\n";
        let block = Block {
            id: "t".into(),
            lang: "javascript".into(),
            text: js,
        };
        let mut colors = colorize_block(&block, &th, ss).unwrap();
        let ms = js.find("msg").unwrap();
        let me = ms + "msg".len();
        assert!(
            colors[0][ms..me].iter().all(|c| c.is_none()),
            "syntactic JS leaves the key plain (the bug this overlay fixes)"
        );
        apply_semantic(
            &mut colors,
            js,
            &[(ms, me, "variable.other.property".into())],
            &th,
        );
        assert!(
            colors[0][ms..me]
                .iter()
                .all(|c| c.map(|c| c.to_hex()) == Some("#9cdcfe".into())),
            "dark-plus colors variable.other.property like TS's syntactic key"
        );
    }
}
