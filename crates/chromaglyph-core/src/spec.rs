//! Input specification types: colors, themes, code blocks and tokens.
//!
//! The pipeline between crates and (later) the napi bindings is the JSON spec
//! `{ font, themes[], blocks[]: { id, lang, text } }`; these types are its
//! in-memory form.

use std::collections::HashMap;

/// An 8-bit RGBA color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Color {
            red,
            green,
            blue,
            alpha: 255,
        }
    }

    /// `#rrggbb` (alpha forced to 255).
    pub fn from_hex6(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Color::rgb(r, g, b))
    }

    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }
}

/// A token scope label produced by the tokenizer, e.g. `keyword`, `string`.
pub type Scope<'a> = &'a str;

/// One highlighted span inside a block's text (byte offsets, `end > start`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token<'a> {
    pub start: usize,
    pub end: usize,
    pub scope: Scope<'a>,
}

/// A scope -> color mapping. `None` means "leave on the base foreground
/// color" (no substitution, no palette entry).
pub type ScopeColors = HashMap<String, Option<Color>>;

/// A named theme. M1 ships exactly one built-in theme; M2 loads TextMate /
/// VS Code JSON themes and stacks several themes as CPAL palettes.
#[derive(Clone, Debug)]
pub struct Theme {
    pub name: String,
    pub scopes: ScopeColors,
}

impl Theme {
    /// Built-in M1 theme (VS Code Dark+ inspired), for the demo tokenizer's
    /// scope names. Punctuation/whitespace stay on the base foreground.
    pub fn builtin_dark() -> Self {
        let mut scopes = ScopeColors::new();
        scopes.insert("keyword".into(), Some(Color::rgb(0x56, 0x9c, 0xd6)));
        scopes.insert("string".into(), Some(Color::rgb(0xce, 0x91, 0x78)));
        scopes.insert("comment".into(), Some(Color::rgb(0x6a, 0x99, 0x55)));
        scopes.insert("number".into(), Some(Color::rgb(0xb5, 0xce, 0xa8)));
        scopes.insert("identifier".into(), Some(Color::rgb(0x9c, 0xdc, 0xfe)));
        scopes.insert("punctuation".into(), None);
        scopes.insert("whitespace".into(), None);
        scopes.insert("unknown".into(), None);
        Theme {
            name: "chromaglyph-dark".into(),
            scopes,
        }
    }

    pub fn color_for(&self, scope: &str) -> Option<Option<Color>> {
        self.scopes.get(scope).copied()
    }
}

/// One code block to compile into the font. Each block becomes an independent
/// shaping run at render time (a separate DOM text node), which is why rule
/// contexts never cross block boundaries.
#[derive(Clone, Debug)]
pub struct Block<'a> {
    pub id: String,
    pub lang: String,
    pub text: &'a str,
}
