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

    /// `#rrggbb` or `#rrggbbaa`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#')?;
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color::rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Color {
                    red: r,
                    green: g,
                    blue: b,
                    alpha: a,
                })
            }
            _ => None,
        }
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
