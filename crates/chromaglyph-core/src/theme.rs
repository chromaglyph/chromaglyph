//! Theme loading: VS Code JSON themes (`.json`, JSONC) and TextMate themes
//! (`.tmTheme`), converted to a syntect `Theme` so scope→color resolution
//! reuses syntect's selector engine.
//!
//! Multiple themes compile into one font with one CPAL palette per theme
//! (see `compile`'s slot model); CSS `@font-palette-values` selects between
//! them, e.g. dark/light following `prefers-color-scheme`.

use std::path::{Path, PathBuf};

use syntect::highlighting::{
    Color as SyntectColor, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};

use crate::error::BuildError;
use crate::spec::Color;

const DARK_PLUS: &str = include_str!("../../../themes/dark_plus.json");
const LIGHT_PLUS: &str = include_str!("../../../themes/light_plus.json");

/// A theme ready for highlighting: the syntect theme plus the editor-level
/// colors VS Code keeps outside `tokenColors`.
#[derive(Clone, Debug)]
pub struct ThemeSpec {
    pub name: String,
    /// Default text color; palette entries fall back to it when a token is
    /// uncolored in this theme but colored in another.
    pub foreground: Color,
    /// Editor background; goes into CSS, never into the font.
    pub background: Option<Color>,
    pub theme: Theme,
}

impl ThemeSpec {
    /// Built-in themes, vendored from VS Code's `theme-defaults` (MIT).
    /// `dark-plus` and `light-plus` form the default light/dark pair.
    pub fn builtin(name: &str) -> Result<ThemeSpec, BuildError> {
        let (json, fallback_name) = match name {
            "dark-plus" | "dark+" | "dark" => (DARK_PLUS, "Dark+"),
            "light-plus" | "light+" | "light" => (LIGHT_PLUS, "Light+"),
            _ => {
                return Err(BuildError::Theme(format!(
                    "unknown builtin theme {name:?}; available: dark-plus, light-plus"
                )))
            }
        };
        let mut spec = parse_vscode_json(json, &mut IncludeFiles::builtin())?;
        if spec.name.is_empty() {
            spec.name = fallback_name.to_string();
        }
        Ok(spec)
    }

    /// Load from a file: `.tmTheme` (TextMate plist) or `.json` (VS Code).
    pub fn load_file(path: &Path) -> Result<ThemeSpec, BuildError> {
        let bytes = std::fs::read(path)
            .map_err(|e| BuildError::Theme(format!("cannot read {}: {e}", path.display())))?;
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "tmtheme" | "plist" => {
                let theme = syntect::highlighting::ThemeSet::load_from_reader(
                    &mut std::io::Cursor::new(&bytes),
                )
                .map_err(|e| BuildError::Theme(format!("{}: {e}", path.display())))?;
                Ok(spec_from_syntect(theme, path.display().to_string()))
            }
            _ => {
                let json = String::from_utf8_lossy(&bytes).into_owned();
                let dir = path.parent().map(Path::to_path_buf);
                let mut spec = parse_vscode_json(&json, &mut IncludeFiles::fs(dir))?;
                if spec.name.is_empty() {
                    spec.name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                }
                Ok(spec)
            }
        }
    }
}

/// Resolve a `--theme` argument: builtin name or path.
pub fn load_theme_arg(arg: &str) -> Result<ThemeSpec, BuildError> {
    let as_path = PathBuf::from(arg);
    if as_path.exists() {
        ThemeSpec::load_file(&as_path)
    } else {
        ThemeSpec::builtin(arg)
    }
}

fn spec_from_syntect(theme: Theme, fallback_name: String) -> ThemeSpec {
    let name = theme
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .unwrap_or(fallback_name);
    let foreground = theme
        .settings
        .foreground
        .map(|c| Color::rgb(c.r, c.g, c.b))
        .unwrap_or_else(|| Color::rgb(0xd4, 0xd4, 0xd4));
    let background = theme.settings.background.map(|c| Color::rgb(c.r, c.g, c.b));
    ThemeSpec {
        name,
        foreground,
        background,
        theme,
    }
}

/// How `include` references resolve: vendored builtins vs sibling files.
enum IncludeFiles {
    Builtin,
    Fs(Option<PathBuf>),
}

impl IncludeFiles {
    fn builtin() -> Self {
        IncludeFiles::Builtin
    }

    fn fs(dir: Option<PathBuf>) -> Self {
        IncludeFiles::Fs(dir)
    }

    fn read(&mut self, include: &str) -> Result<String, BuildError> {
        let file = include.rsplit('/').next().unwrap_or(include);
        match self {
            IncludeFiles::Builtin => match file {
                "dark_vs.json" => Ok(include_str!("../../../themes/dark_vs.json").to_string()),
                "light_vs.json" => Ok(include_str!("../../../themes/light_vs.json").to_string()),
                _ => Err(BuildError::Theme(format!(
                    "builtin themes cannot include {include:?}"
                ))),
            },
            IncludeFiles::Fs(dir) => {
                let dir = dir.clone().ok_or_else(|| {
                    BuildError::Theme(format!("cannot resolve include {include:?}"))
                })?;
                let text = std::fs::read_to_string(dir.join(file)).map_err(|e| {
                    BuildError::Theme(format!("cannot read included {include:?}: {e}"))
                })?;
                Ok(text)
            }
        }
    }
}

/// Parse a VS Code color theme (JSONC) into a `ThemeSpec`, following
/// `include` chains (included rules merge with lower priority).
fn parse_vscode_json(jsonc: &str, includes: &mut IncludeFiles) -> Result<ThemeSpec, BuildError> {
    #[derive(serde::Deserialize)]
    struct VsTheme {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        include: Option<String>,
        #[serde(default, rename = "tokenColors")]
        token_colors: Vec<VsTokenColor>,
        #[serde(default)]
        colors: std::collections::HashMap<String, String>,
    }
    #[derive(serde::Deserialize)]
    struct VsTokenColor {
        #[serde(default)]
        scope: Option<serde_json::Value>,
        #[serde(default)]
        settings: VsSettings,
    }
    #[derive(serde::Deserialize, Default)]
    struct VsSettings {
        #[serde(default)]
        foreground: Option<String>,
    }

    fn scope_strings(scope: &Option<serde_json::Value>) -> Vec<String> {
        match scope {
            None => Vec::new(),
            Some(serde_json::Value::String(s)) => {
                s.split(',').map(str::trim).map(String::from).collect()
            }
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str())
                .flat_map(|s| s.split(',').map(str::trim))
                .map(String::from)
                .collect(),
            Some(_) => Vec::new(),
        }
    }

    fn colors_of(theme: &VsTheme) -> (Option<Color>, Option<Color>) {
        let fg = theme
            .colors
            .get("editor.foreground")
            .and_then(|s| Color::from_hex(s));
        let bg = theme
            .colors
            .get("editor.background")
            .and_then(|s| Color::from_hex(s));
        (fg, bg)
    }

    // merge chain: [this, include-1, ..., include-n] (root first)
    let mut chain: Vec<VsTheme> = Vec::new();
    let mut next = Some(jsonc.to_string());
    while let Some(text) = next.take() {
        let theme: VsTheme = serde_json::from_str(&strip_jsonc(&text))
            .map_err(|e| BuildError::Theme(format!("invalid VS Code theme JSON: {e}")))?;
        next = theme
            .include
            .as_deref()
            .and_then(|inc| includes.read(inc).ok());
        chain.push(theme);
    }
    let this = chain.remove(0);

    let mut items: Vec<ThemeItem> = Vec::new();
    // included rules first: syntect applies modifiers in order, so the
    // including theme's (later) rules win ties, matching VS Code
    for theme in &chain {
        for tc in &theme.token_colors {
            push_items(
                &mut items,
                &scope_strings(&tc.scope),
                &tc.settings.foreground,
            );
        }
    }
    for tc in &this.token_colors {
        push_items(
            &mut items,
            &scope_strings(&tc.scope),
            &tc.settings.foreground,
        );
    }

    // editor colors: including theme wins
    let (mut fg, mut bg) = (None, None);
    for theme in &chain {
        let (f, b) = colors_of(theme);
        fg = f.or(fg);
        bg = b.or(bg);
    }
    let (f, b) = colors_of(&this);
    fg = f.or(fg);
    bg = b.or(bg);

    let foreground = fg.unwrap_or_else(|| Color::rgb(0xd4, 0xd4, 0xd4));
    let syntect_theme = Theme {
        name: this.name.clone().filter(|n| !n.is_empty()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(SyntectColor {
                r: foreground.red,
                g: foreground.green,
                b: foreground.blue,
                a: 255,
            }),
            background: bg.map(|c| SyntectColor {
                r: c.red,
                g: c.green,
                b: c.blue,
                a: 255,
            }),
            ..Default::default()
        },
        scopes: items,
    };
    let name = this
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .unwrap_or_default();
    Ok(ThemeSpec {
        name,
        foreground,
        background: bg,
        theme: syntect_theme,
    })
}

fn push_items(items: &mut Vec<ThemeItem>, scopes: &[String], foreground: &Option<String>) {
    let Some(fg) = foreground else { return };
    let Some(color) = Color::from_hex(fg) else {
        return;
    };
    for scope in scopes {
        let Ok(selectors) = scope.parse::<ScopeSelectors>() else {
            continue;
        };
        items.push(ThemeItem {
            scope: selectors,
            style: StyleModifier {
                foreground: Some(SyntectColor {
                    r: color.red,
                    g: color.green,
                    b: color.blue,
                    a: color.alpha,
                }),
                background: None,
                font_style: None,
            },
        });
    }
}

/// Strip `//` and `/* */` comments and trailing commas from JSONC.
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for c2 in chars.by_ref() {
                    if c2 == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for c2 in chars.by_ref() {
                    if prev == '*' && c2 == '/' {
                        break;
                    }
                    prev = c2;
                }
            }
            _ => out.push(c),
        }
    }
    // trailing commas: ",]" / ",}" (whitespace allowed between)
    let mut trimmed = String::with_capacity(out.len());
    let bytes: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == ',' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_whitespace() {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == '}' || bytes[j] == ']') {
                i += 1; // drop the comma
                continue;
            }
        }
        trimmed.push(c);
        i += 1;
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_dark_plus_loads() {
        let spec = ThemeSpec::builtin("dark-plus").unwrap();
        assert_eq!(spec.name, "Dark+");
        assert_eq!(spec.foreground.to_hex(), "#d4d4d4");
        assert_eq!(
            spec.background.map(|c| c.to_hex()),
            Some("#1e1e1e".to_string())
        );
        assert!(
            spec.theme.scopes.len() > 50,
            "include chain merged: {}",
            spec.theme.scopes.len()
        );
    }

    #[test]
    fn jsonc_stripper_handles_comments_and_trailing_commas() {
        let src = r#"{
            // comment
            "a": 1, /* block */
            "b": [1, 2, 3,],
            "c": "http://x", // url string untouched
        }"#;
        let v: serde_json::Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["b"][2], 3);
        assert_eq!(v["c"], "http://x");
    }

    #[test]
    fn vscode_theme_from_memory() {
        let json = r##"{
            "name": "Test",
            "tokenColors": [
                { "scope": ["keyword", "storage.type"], "settings": { "foreground": "#ff0000" } },
                { "scope": "string.quoted", "settings": { "foreground": "#00ff00" } }
            ]
        }"##;
        let mut includes = IncludeFiles::builtin();
        let spec = parse_vscode_json(json, &mut includes).unwrap();
        assert_eq!(spec.name, "Test");
        assert_eq!(spec.theme.scopes.len(), 3);
    }
}
