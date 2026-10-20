//! Theme loading: VS Code JSON themes (`.json`, JSONC) and TextMate themes
//! (`.tmTheme`), converted to a syntect `Theme` so scope→color resolution
//! reuses syntect's selector engine.
//!
//! Builtin themes are shiki's `tm-themes` collection (VS Code theme files
//! with `include` chains flattened upstream), vendored byte-for-byte under
//! `themes/tm/` by `scripts/vendor-themes.mjs` and gzipped into the binary
//! by `build.rs`. [`builtin_themes`] is the registry.
//!
//! Multiple themes compile into one font with one CPAL palette per theme
//! (see `compile`'s slot model); CSS `@font-palette-values` selects between
//! them, e.g. dark/light following `prefers-color-scheme`.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use syntect::highlighting::{
    Color as SyntectColor, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};

use crate::error::BuildError;
use crate::spec::Color;

/// Dark or light, as the theme declares itself (shiki's `type` field).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
}

impl ThemeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeKind::Dark => "dark",
            ThemeKind::Light => "light",
        }
    }
}

/// One vendored theme: registry metadata plus the upstream JSON, gzipped.
#[derive(Debug)]
pub struct BuiltinTheme {
    /// Stable id, also the `ThemeSpec::name` (`dark-plus`, `github-light`, …).
    pub slug: &'static str,
    pub display_name: &'static str,
    pub kind: ThemeKind,
    /// SPDX id; only permissive licenses are vendored.
    pub license: &'static str,
    pub license_url: &'static str,
    /// Upstream file at the pinned commit.
    pub source: &'static str,
    json_gz: &'static [u8],
}

impl BuiltinTheme {
    /// The upstream theme JSON, byte-for-byte.
    pub fn json(&self) -> Result<String, BuildError> {
        let mut json = String::new();
        flate2::read::GzDecoder::new(self.json_gz)
            .read_to_string(&mut json)
            .map_err(|e| BuildError::Theme(format!("builtin theme {}: {e}", self.slug)))?;
        Ok(json)
    }

    pub fn load(&self) -> Result<ThemeSpec, BuildError> {
        let mut spec = parse_vscode_json(&self.json()?, &mut IncludeFiles::Embedded)?;
        spec.name = self.slug.to_string();
        Ok(spec)
    }
}

mod registry {
    use super::{BuiltinTheme, ThemeKind};
    include!(concat!(env!("OUT_DIR"), "/builtin_themes.rs"));
}

/// Version of the `tm-themes` package the builtins were vendored from.
pub const TM_THEMES_VERSION: &str = registry::TM_THEMES_VERSION;

/// Every builtin theme, sorted by slug.
pub fn builtin_themes() -> &'static [BuiltinTheme] {
    registry::BUILTINS
}

/// Look a builtin up by slug or alias (`dark`, `dark+` → `dark-plus`;
/// `light`, `light+` → `light-plus`).
pub fn builtin_theme(name: &str) -> Option<&'static BuiltinTheme> {
    let slug = match name {
        "dark" | "dark+" => "dark-plus",
        "light" | "light+" => "light-plus",
        other => other,
    };
    registry::BUILTINS.iter().find(|t| t.slug == slug)
}

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
    /// A builtin theme by slug or alias; `dark-plus` and `light-plus` form
    /// the default dark/light pair. See [`builtin_themes`] for the list.
    pub fn builtin(name: &str) -> Result<ThemeSpec, BuildError> {
        match builtin_theme(name) {
            Some(theme) => theme.load(),
            None => {
                let available: Vec<&str> = builtin_themes().iter().map(|t| t.slug).collect();
                Err(BuildError::Theme(format!(
                    "unknown builtin theme {name:?} (and no such file); builtins: {}",
                    available.join(", ")
                )))
            }
        }
    }

    /// Resolve a dotted TextMate scope's foreground via syntect's selector
    /// engine, the way the highlighting pipeline would.
    pub fn scope_color(&self, scope: &str) -> Option<Color> {
        let highlighter = syntect::highlighting::Highlighter::new(&self.theme);
        let mut stack = syntect::parsing::ScopeStack::new();
        stack.push(syntect::parsing::Scope::new(scope).ok()?);
        highlighter
            .style_mod_for_stack(&stack.scopes)
            .foreground
            .map(|c| Color {
                red: c.r,
                green: c.g,
                blue: c.b,
                alpha: c.a,
            })
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
    let background = theme.settings.background.map(|c| Color::rgb(c.r, c.g, c.b));
    let foreground = theme
        .settings
        .foreground
        .map(|c| Color::rgb(c.r, c.g, c.b))
        .unwrap_or_else(|| default_foreground(None, background));
    ThemeSpec {
        name,
        foreground,
        background,
        theme,
    }
}

/// How `include` references resolve. Builtins are flattened upstream and
/// embedded one file each, so an include there has nothing to resolve
/// against and is an error (the vendor script refuses such files too).
enum IncludeFiles {
    Embedded,
    Fs(Option<PathBuf>),
}

impl IncludeFiles {
    fn fs(dir: Option<PathBuf>) -> Self {
        IncludeFiles::Fs(dir)
    }

    fn read(&mut self, include: &str) -> Result<String, BuildError> {
        let file = include.rsplit('/').next().unwrap_or(include);
        match self {
            IncludeFiles::Embedded => Err(BuildError::Theme(format!(
                "builtin themes cannot include {include:?}"
            ))),
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
        /// `"dark"` / `"light"`: not part of VS Code's file format (the
        /// editor takes it from the extension manifest) but written into
        /// every theme by shiki's `tm-themes` collection.
        #[serde(default, rename = "type")]
        theme_type: Option<String>,
        #[serde(default)]
        include: Option<String>,
        #[serde(default, rename = "tokenColors")]
        token_colors: Vec<VsTokenColor>,
        /// Values are usually hex strings, but themes in the wild also put
        /// `null` and arrays here and VS Code accepts them.
        #[serde(default)]
        colors: std::collections::HashMap<String, serde_json::Value>,
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
        #[serde(default)]
        background: Option<String>,
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
        let color = |key: &str| {
            theme
                .colors
                .get(key)
                .and_then(serde_json::Value::as_str)
                .and_then(Color::from_hex)
        };
        (color("editor.foreground"), color("editor.background"))
    }

    // merge chain: [this, include-1, ..., include-n] (root first). An
    // include that cannot be read is an error, as in VS Code: silently
    // dropping it would lose the base rules and recolor everything.
    let mut chain: Vec<VsTheme> = Vec::new();
    let mut next = Some(jsonc.to_string());
    while let Some(text) = next.take() {
        let theme: VsTheme = serde_json::from_str(&strip_jsonc(&text))
            .map_err(|e| BuildError::Theme(format!("invalid VS Code theme JSON: {e}")))?;
        if let Some(inc) = theme.include.as_deref() {
            if chain.len() >= 8 {
                return Err(BuildError::Theme(format!(
                    "include chain too deep at {inc:?} (cycle?)"
                )));
            }
            next = Some(includes.read(inc)?);
        }
        chain.push(theme);
    }
    let this = chain.remove(0);

    let mut items: Vec<ThemeItem> = Vec::new();
    // a scope-less rule sets the editor defaults instead of styling a scope
    let (mut rule_fg, mut rule_bg) = (None, None);
    // deepest include first: syntect applies modifiers in order, so later
    // rules win ties and each theme overrides what it includes, matching
    // VS Code
    for theme in chain.iter().rev().chain(std::iter::once(&this)) {
        for tc in &theme.token_colors {
            let scopes = scope_strings(&tc.scope);
            if scopes.iter().all(|s| s.is_empty()) {
                let fg = tc.settings.foreground.as_deref().and_then(Color::from_hex);
                let bg = tc.settings.background.as_deref().and_then(Color::from_hex);
                rule_fg = fg.or(rule_fg);
                rule_bg = bg.or(rule_bg);
            } else {
                push_items(&mut items, &scopes, &tc.settings.foreground);
            }
        }
    }

    // editor colors: `colors.editor.*` (including theme first), then the
    // scope-less rule, then VS Code's per-type default
    let (mut fg, mut bg) = (None, None);
    for theme in std::iter::once(&this).chain(chain.iter()) {
        let (f, b) = colors_of(theme);
        fg = fg.or(f);
        bg = bg.or(b);
    }
    let background = bg.or(rule_bg);
    let theme_type = std::iter::once(&this)
        .chain(chain.iter())
        .find_map(|t| t.theme_type.as_deref());
    let foreground = fg
        .or(rule_fg)
        .unwrap_or_else(|| default_foreground(theme_type, background));

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
            background: background.map(|c| SyntectColor {
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
        background,
        theme: syntect_theme,
    })
}

/// VS Code's `editor.foreground` default for themes that set none: by theme
/// type when the file says, else judged from the background.
fn default_foreground(theme_type: Option<&str>, background: Option<Color>) -> Color {
    let light = match theme_type {
        Some(t) => t.eq_ignore_ascii_case("light"),
        None => background.is_some_and(|c| c.is_light()),
    };
    if light {
        Color::rgb(0x33, 0x33, 0x33)
    } else {
        Color::rgb(0xd4, 0xd4, 0xd4)
    }
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
        assert_eq!(spec.name, "dark-plus");
        assert_eq!(spec.foreground.to_hex(), "#d4d4d4");
        assert_eq!(
            spec.background.map(|c| c.to_hex()),
            Some("#1e1e1e".to_string())
        );
        assert!(
            spec.theme.scopes.len() > 150,
            "flattened dark_vs + dark_plus rules: {}",
            spec.theme.scopes.len()
        );
        assert_eq!(
            spec.scope_color("keyword.control").map(|c| c.to_hex()),
            Some("#c586c0".into())
        );
    }

    #[test]
    fn builtin_registry_and_aliases() {
        let all = builtin_themes();
        assert!(all.len() >= 60, "vendored {} themes", all.len());
        assert!(
            all.windows(2).all(|w| w[0].slug < w[1].slug),
            "sorted by slug"
        );
        assert!(all
            .iter()
            .all(|t| !t.display_name.is_empty() && !t.source.is_empty()));
        assert!(all
            .iter()
            .all(|t| matches!(t.license, "MIT" | "Apache-2.0")));
        assert!(
            builtin_theme("aurora-x").is_none(),
            "GPL theme must not ship"
        );
        for alias in ["dark", "dark+", "dark-plus"] {
            assert_eq!(builtin_theme(alias).unwrap().slug, "dark-plus");
        }
        for alias in ["light", "light+", "light-plus"] {
            assert_eq!(builtin_theme(alias).unwrap().slug, "light-plus");
        }
        assert_eq!(builtin_theme("github-dark").unwrap().kind, ThemeKind::Dark);
        assert_eq!(
            builtin_theme("github-light").unwrap().kind,
            ThemeKind::Light
        );
        assert!(!TM_THEMES_VERSION.is_empty());

        let err = ThemeSpec::builtin("nope").unwrap_err().to_string();
        assert!(err.contains("\"nope\"") && err.contains("dark-plus") && err.contains("nord"));
    }

    #[test]
    fn builtin_json_is_upstream_verbatim() {
        let t = builtin_theme("nord").unwrap();
        let json = t.json().unwrap();
        let on_disk = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("themes/tm/nord.json"),
        )
        .unwrap();
        assert_eq!(json, on_disk);
    }

    #[test]
    fn unreadable_include_is_an_error() {
        let json = r##"{ "include": "./missing.json", "tokenColors": [] }"##;
        let err = parse_vscode_json(
            json,
            &mut IncludeFiles::fs(Some(PathBuf::from("/nonexistent"))),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("missing.json"), "{err}");
        let err = parse_vscode_json(json, &mut IncludeFiles::Embedded)
            .unwrap_err()
            .to_string();
        assert!(err.contains("builtin themes cannot include"), "{err}");
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
        let spec = parse_vscode_json(json, &mut IncludeFiles::Embedded).unwrap();
        assert_eq!(spec.name, "Test");
        assert_eq!(spec.theme.scopes.len(), 3);
    }

    fn parse(json: &str) -> ThemeSpec {
        parse_vscode_json(json, &mut IncludeFiles::Embedded).unwrap()
    }

    #[test]
    fn colors_map_tolerates_non_string_values() {
        // github-dark-* ship an array, night-owl ships nulls; VS Code loads both
        let spec = parse(
            r##"{
            "colors": {
                "symbolIcon.constantForeground": ["#aff5b4", "#7ee787"],
                "editor.lineHighlightBorder": null,
                "editor.foreground": "#e1e4e8",
                "editor.background": "#24292e"
            },
            "tokenColors": []
        }"##,
        );
        assert_eq!(spec.foreground.to_hex(), "#e1e4e8");
        assert_eq!(spec.background.map(|c| c.to_hex()), Some("#24292e".into()));
    }

    #[test]
    fn short_hex_colors_apply() {
        let spec = parse(
            r##"{
            "colors": { "editor.foreground": "#FFF", "editor.background": "#000" },
            "tokenColors": [
                { "scope": "keyword", "settings": { "foreground": "#f00" } }
            ]
        }"##,
        );
        assert_eq!(spec.foreground.to_hex(), "#ffffff");
        assert_eq!(spec.background.map(|c| c.to_hex()), Some("#000000".into()));
        assert_eq!(
            spec.scope_color("keyword.control").map(|c| c.to_hex()),
            Some("#ff0000".into())
        );
    }

    #[test]
    fn scopeless_rule_sets_editor_defaults() {
        // no `colors`: the scope-less rule is the default (min-dark, gruvbox, ...)
        let spec = parse(
            r##"{
            "tokenColors": [
                { "settings": { "foreground": "#b392f0", "background": "#1f1f1f" } },
                { "scope": "keyword", "settings": { "foreground": "#f97583" } }
            ]
        }"##,
        );
        assert_eq!(spec.foreground.to_hex(), "#b392f0");
        assert_eq!(spec.background.map(|c| c.to_hex()), Some("#1f1f1f".into()));
        assert_eq!(
            spec.theme.scopes.len(),
            1,
            "scope-less rule is not a scope rule"
        );
        assert_eq!(
            spec.scope_color("comment"),
            None,
            "uncolored scopes stay on the default foreground, not on a rule"
        );

        // `colors.editor.foreground` outranks the scope-less rule (slack-ochin)
        let spec = parse(
            r##"{
            "colors": { "editor.foreground": "#000" },
            "tokenColors": [ { "scope": "", "settings": { "foreground": "#002339" } } ]
        }"##,
        );
        assert_eq!(spec.foreground.to_hex(), "#000000");
    }

    #[test]
    fn default_foreground_follows_theme_type_then_background() {
        let fg = |json: &str| parse(json).foreground.to_hex();
        // shiki's `type` field decides when present
        assert_eq!(
            fg(r##"{ "type": "light", "colors": { "editor.background": "#fdf0ed" } }"##),
            "#333333"
        );
        assert_eq!(
            fg(r##"{ "type": "dark", "colors": { "editor.background": "#fafafa" } }"##),
            "#d4d4d4"
        );
        // otherwise judge from the background; no background means dark
        assert_eq!(
            fg(r##"{ "colors": { "editor.background": "#ffffff" } }"##),
            "#333333"
        );
        assert_eq!(
            fg(r##"{ "colors": { "editor.background": "#1e1e1e" } }"##),
            "#d4d4d4"
        );
        assert_eq!(fg(r##"{ "tokenColors": [] }"##), "#d4d4d4");
    }

    #[test]
    fn builtin_light_plus_keeps_explicit_foreground() {
        let spec = ThemeSpec::builtin("light-plus").unwrap();
        assert_eq!(spec.foreground.to_hex(), "#000000");
        assert_eq!(spec.background.map(|c| c.to_hex()), Some("#ffffff".into()));
    }
}
