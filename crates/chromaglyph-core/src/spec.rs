//! Input specification types: colors and code blocks.
//!
//! The pipeline between crates and (later) the napi bindings is the JSON spec
//! `{ font, themes[], blocks[]: { id, lang, text } }`; these types are its
//! in-memory form. Themes are resolved by the tokenizer stage (syntect):
//! what reaches `compile` is a concrete color per character per theme.

/// An 8-bit RGBA color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

    /// CSS hex forms: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#')?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let wide = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        let short = |i: usize| {
            u8::from_str_radix(&hex[i..i + 1], 16)
                .ok()
                .map(|d| d * 0x11)
        };
        let (red, green, blue, alpha) = match hex.len() {
            3 => (short(0)?, short(1)?, short(2)?, 255),
            4 => (short(0)?, short(1)?, short(2)?, short(3)?),
            6 => (wide(0)?, wide(2)?, wide(4)?, 255),
            8 => (wide(0)?, wide(2)?, wide(4)?, wide(6)?),
            _ => return None,
        };
        Some(Color {
            red,
            green,
            blue,
            alpha,
        })
    }

    /// Perceived brightness above mid-gray; decides which VS Code default
    /// foreground a theme without an explicit one gets.
    pub fn is_light(&self) -> bool {
        let luma = 0.299 * f32::from(self.red)
            + 0.587 * f32::from(self.green)
            + 0.114 * f32::from(self.blue);
        luma > 127.5
    }

    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }
}

/// One code block to compile into the font. Each block becomes an independent
/// shaping run at render time (a separate DOM text node), which is why rule
/// contexts never cross block boundaries.
///
/// `lang` names the source syntax: a file extension (`rs`, `ts`, `py`, …),
/// one of the aliases resolved by [`crate::tokenize`], or `plain`.
#[derive(Clone, Debug)]
pub struct Block<'a> {
    pub id: String,
    pub lang: String,
    pub text: &'a str,
}
