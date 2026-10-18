//! Error types for the chromaglyph build/verify pipeline.

use std::fmt;

/// Fatal errors produced while building or verifying a font.
#[derive(Debug)]
pub enum BuildError {
    /// The input font could not be parsed at all.
    FontParse(String),
    /// The font's outlines (or organization) are beyond what the
    /// pipeline can rework: CFF2/variable-OTF, CID-keyed CFF, bitmap-only
    /// fonts, multi-font CFF sets.
    UnsupportedFont(String),
    /// The font is missing a table we require.
    MissingTable(&'static str),
    /// Tokens overlap or do not lie inside the block text.
    BadTokens(String),
    /// A write-fonts table failed to serialize.
    TableWrite(String),
    /// The rustybuzz oracle disagreed with the intended highlighting.
    VerifyFailed(Vec<String>),
    /// WOFF wrapping/unwrapping failure.
    Woff(String),
    /// Theme loading/parsing failure.
    Theme(String),
    /// Tokenizer failure (unknown language, syntect error).
    Tokenize(String),
    /// I/O or misc failure with context.
    Io(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::FontParse(msg) => write!(f, "cannot parse font: {msg}"),
            BuildError::UnsupportedFont(msg) => write!(f, "unsupported font: {msg}"),
            BuildError::MissingTable(tag) => write!(f, "font is missing required table '{tag}'"),
            BuildError::BadTokens(msg) => write!(f, "invalid token list: {msg}"),
            BuildError::TableWrite(msg) => write!(f, "table serialization failed: {msg}"),
            BuildError::VerifyFailed(failures) => {
                write!(f, "verify failed with {} problem(s)", failures.len())?;
                for failure in failures.iter().take(10) {
                    write!(f, "\n  - {failure}")?;
                }
                Ok(())
            }
            BuildError::Woff(msg) => write!(f, "woff error: {msg}"),
            BuildError::Theme(msg) => write!(f, "theme error: {msg}"),
            BuildError::Tokenize(msg) => write!(f, "tokenize error: {msg}"),
            BuildError::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<read_fonts::ReadError> for BuildError {
    fn from(err: read_fonts::ReadError) -> Self {
        BuildError::FontParse(err.to_string())
    }
}
