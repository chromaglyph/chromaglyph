//! Byte-level CFF (v1) surgery: append zero-outline glyphs to a
//! name-keyed CFF table.
//!
//! The pipeline's colored copies are *empty* glyphs whose COLR layer
//! references a base glyph (see `fontbuild`); TrueType gets them as empty
//! glyf entries, CFF gets them as bare `endchar` charstrings. Appending
//! glyphs to a CFF table means growing the CharStrings INDEX (and the
//! charset, which must cover every glyph), which shifts every structure
//! that follows it — so the whole table is re-laid-out with the Top DICT
//! offset operands recomputed. Everything else (Name/String/GSubr
//! indexes, the Private DICT and its local subrs) is copied byte-for-byte.
//!
//! Scope: name-keyed fonts only (the normal shape of an OpenType CFF
//! font: Source Code Pro, STIX, …). CID-keyed fonts (Top DICT `ROS`,
//! FDArray/FDSelect with per-glyph font dicts that carry their own
//! absolute Private offsets) are rejected in `parse` — patching those is
//! more surgery than v1 wants. CFF2 (variable OTF outlines) needs real
//! instancing and is rejected before this module runs.

use crate::error::BuildError;

/// Byte positions of a CFF table's structures (offsets into the table).
#[derive(Debug)]
pub struct Layout {
    /// End of the header / start of the Name index.
    pub after_header: usize,
    pub name_index: (usize, usize),
    pub top_dict_index: (usize, usize),
    pub string_index: (usize, usize),
    pub global_subrs: (usize, usize),
    /// Predefined encodings (operand 0/1) carry no bytes; a custom
    /// encoding is a blob that must ride along.
    pub encoding: Option<(usize, usize)>,
    /// Charset structure. Offset 0 (ISOAdobe) is rejected: the charset
    /// must be extended, so it has to exist as bytes.
    pub charset: Option<(usize, usize)>,
    pub charstrings: (usize, usize),
    /// Private DICT plus its trailing local subrs, copied as one blob so
    /// the dict's relative Subrs offset stays valid.
    pub private: Option<(usize, usize)>,
    /// Glyph count as the CharStrings INDEX reports it (== maxp.numGlyphs
    /// in any font OTS would accept; load() cross-checks).
    pub num_glyphs: usize,
}

const ENDCHAR: u8 = 0x0e;

/// Parse a CFF table's layout. Rejects CID-keyed fonts and multi-font
/// font sets (OpenType CFF tables contain exactly one font).
pub fn parse(data: &[u8]) -> Result<Layout, BuildError> {
    let err = |msg: &str| BuildError::UnsupportedFont(format!("CFF: {msg}"));
    if data.len() < 4 || data[0] != 1 {
        return Err(err("not a CFF version 1 table"));
    }
    let hdr_size = data[2] as usize;
    if !(4..=data.len()).contains(&hdr_size) {
        return Err(err("bad header size"));
    }
    let mut at = hdr_size;
    let name_index = index_extent(data, at).ok_or_else(|| err("Name index unreadable"))?;
    at = name_index.1;
    let top_dict_index = index_extent(data, at).ok_or_else(|| err("Top DICT index unreadable"))?;
    at = top_dict_index.1;
    let string_index = index_extent(data, at).ok_or_else(|| err("String index unreadable"))?;
    at = string_index.1;
    let global_subrs = index_extent(data, at).ok_or_else(|| err("GSubr index unreadable"))?;

    let top_dicts =
        IndexView::new(data, top_dict_index.0).ok_or_else(|| err("Top DICT index unreadable"))?;
    if top_dicts.count() != 1 {
        return Err(err("font sets with more than one font are not supported"));
    }
    let top_dict = top_dicts.get(0).ok_or_else(|| err("empty Top DICT"))?;

    // Top DICT scan: collect the offset-valued operators. The ROS operand
    // group means a CID font — bail before doing any layout work.
    let mut charset_at: Option<usize> = None;
    let mut encoding_at: Option<usize> = None;
    let mut charstrings_at: Option<usize> = None;
    let mut private_at: Option<(usize, usize)> = None;
    let mut operands: Vec<&[u8]> = Vec::new();
    for token in tokenize_dict(top_dict) {
        match token {
            Token::Operand(bytes) => operands.push(bytes),
            Token::Operator(op) => {
                match op {
                    [12, 30] => {
                        return Err(err(
                            "CID-keyed fonts are not supported yet; build from the TTF or a name-keyed OTF",
                        ))
                    }
                    [15] => charset_at = Some(last_operand(&operands, "charset")?),
                    [16] => {
                        // 0 = StandardEncoding, 1 = ExpertEncoding: no blob
                        let off = last_operand(&operands, "encoding")?;
                        if off > 1 {
                            encoding_at = Some(off);
                        }
                    }
                    [17] => {
                        charstrings_at = Some(last_operand(&operands, "CharStrings")?)
                    }
                    [18] => {
                        // Private takes (size, offset)
                        if operands.len() < 2 {
                            return Err(err("Private operator without size+offset"));
                        }
                        let size =
                            last_operand(&operands[..operands.len() - 1], "Private size")?;
                        let off = last_operand(&operands, "Private offset")?;
                        private_at = Some((off, size));
                    }
                    _ => {}
                }
                operands.clear();
            }
        }
    }

    let charstrings_at = charstrings_at.ok_or_else(|| err("Top DICT has no CharStrings offset"))?;
    let cs_view =
        IndexView::new(data, charstrings_at).ok_or_else(|| err("CharStrings index unreadable"))?;
    let num_glyphs = cs_view.count();
    if num_glyphs == 0 {
        return Err(err("CharStrings index is empty"));
    }
    let charstrings = (charstrings_at, charstrings_at + cs_view.total_len());

    let charset = match charset_at {
        Some(0) | None => return Err(err("charset offset 0 (predefined) cannot be extended")),
        Some(at) => {
            let end = charset_end(data, at, num_glyphs)
                .ok_or_else(|| err("charset does not cover every glyph"))?;
            Some((at, end))
        }
    };

    let encoding = match encoding_at {
        Some(at) => {
            let end = encoding_end(data, at).ok_or_else(|| err("custom encoding unreadable"))?;
            Some((at, end))
        }
        None => None,
    };

    // the Private blob runs to the end of its local subrs (whose offset
    // is relative to the Private DICT start, so the pair must move
    // together)
    let private = match private_at {
        None => None,
        Some((off, size)) => {
            if off + size > data.len() {
                return Err(err("Private DICT out of bounds"));
            }
            let mut end = off + size;
            let mut operands: Vec<&[u8]> = Vec::new();
            for token in tokenize_dict(&data[off..off + size]) {
                match token {
                    Token::Operand(bytes) => operands.push(bytes),
                    Token::Operator([19]) => {
                        let rel = last_operand(&operands, "Subrs")?;
                        let subrs_at = off + rel;
                        let subrs = IndexView::new(data, subrs_at)
                            .ok_or_else(|| err("local Subrs index unreadable"))?;
                        end = end.max(subrs_at + subrs.total_len());
                    }
                    Token::Operator(_) => operands.clear(),
                }
            }
            Some((off, end))
        }
    };

    Ok(Layout {
        after_header: hdr_size,
        name_index,
        top_dict_index,
        string_index,
        global_subrs,
        encoding,
        charset,
        charstrings,
        private,
        num_glyphs,
    })
}

/// Append `count` zero-outline glyphs (bare `endchar` charstrings) and
/// return the re-laid-out CFF table.
pub fn append_empty_glyphs(
    data: &[u8],
    layout: &Layout,
    count: usize,
) -> Result<Vec<u8>, BuildError> {
    if count == 0 {
        return Ok(data.to_vec());
    }
    let err = |msg: &str| BuildError::UnsupportedFont(format!("CFF: {msg}"));

    let (cs_at, cs_end) = match layout.charset {
        // parse() rejects charset-less fonts, so this always matches
        Some(range) => range,
        None => return Err(err("font has no charset to extend")),
    };
    let charset = extend_charset(&data[cs_at..cs_end], layout.num_glyphs, count)
        .ok_or_else(|| err("cannot extend the charset"))?;
    let charstrings = {
        let view = IndexView::new(data, layout.charstrings.0)
            .ok_or_else(|| err("CharStrings index unreadable"))?;
        let mut items: Vec<&[u8]> = Vec::with_capacity(view.count() + count);
        for i in 0..view.count() {
            items.push(
                view.get(i)
                    .ok_or_else(|| err("CharStrings item unreadable"))?,
            );
        }
        items.extend(std::iter::repeat_n(&[ENDCHAR][..], count));
        append_index_items(&items)
    };

    // Re-serialize the Top DICT with every offset operand in fixed 5-byte
    // form (29 + u32) so the dict's serialized length — and therefore the
    // layout that follows it — is the same whether the offsets are
    // placeholders or final. Two passes: measure, then patch for real.
    let top_dict = IndexView::new(data, layout.top_dict_index.0)
        .and_then(|v| v.get(0))
        .ok_or_else(|| err("Top DICT unreadable"))?;

    let encoding = layout.encoding.map(|(a, b)| data[a..b].to_vec());
    let private = layout.private.map(|(a, b)| data[a..b].to_vec());
    let placeholders = OffsetTargets {
        charset: 0,
        encoding: encoding.is_some().then_some(0),
        charstrings: 0,
        private_offset: private.is_some().then_some(0),
    };
    let patched_len = patch_top_dict(top_dict, &placeholders).len();

    // final layout: header, the four leading indexes (Top DICT with its
    // new length), then the offset-valued structures in source order
    let top_dict_index_len = 2 + 1 + 2 + patched_len; // count + offSize + 2 offsets + dict
    let mut at = layout.after_header
        + (layout.name_index.1 - layout.name_index.0)
        + top_dict_index_len
        + (layout.string_index.1 - layout.string_index.0)
        + (layout.global_subrs.1 - layout.global_subrs.0);
    let charset_at = at;
    at += charset.len();
    let encoding_at = encoding.as_ref().map(|enc| {
        let a = at;
        at += enc.len();
        a
    });
    let charstrings_at = at;
    at += charstrings.len();
    let private_at = private.as_ref().map(|_| at);

    let patched = patch_top_dict(
        top_dict,
        &OffsetTargets {
            charset: charset_at as u32,
            encoding: encoding_at.map(|a| a as u32),
            charstrings: charstrings_at as u32,
            private_offset: private_at.map(|a| a as u32),
        },
    );
    debug_assert_eq!(patched.len(), patched_len, "fixed-width operands");

    let mut out = Vec::with_capacity(at + private.as_ref().map_or(0, |p| p.len()));
    out.extend_from_slice(&data[..layout.after_header]);
    out.extend_from_slice(&data[layout.name_index.0..layout.name_index.1]);
    // Top DICT INDEX re-emit with the single (patched) dict
    let last = (1 + patched.len()) as u32;
    let off_size = off_size_for(last as usize);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.push(off_size as u8);
    push_offset(&mut out, 1, off_size);
    push_offset(&mut out, last as usize, off_size);
    out.extend_from_slice(&patched);
    out.extend_from_slice(&data[layout.string_index.0..layout.string_index.1]);
    out.extend_from_slice(&data[layout.global_subrs.0..layout.global_subrs.1]);
    out.extend_from_slice(&charset);
    if let Some(enc) = &encoding {
        out.extend_from_slice(enc);
    }
    out.extend_from_slice(&charstrings);
    if let Some(priv_blob) = &private {
        out.extend_from_slice(priv_blob);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// DICT tokenizing / patching
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Token<'a> {
    Operand(&'a [u8]),
    Operator(&'a [u8]),
}

/// Split a DICT into operand/operator tokens (slices of the input).
fn tokenize_dict(data: &[u8]) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let len = match b0 {
            32..=246 => 1,
            247..=254 => 2,
            28 => 3,
            29 => 5,
            30 => {
                // real number: nibbles until a low nibble of 0xf
                let mut j = i + 1;
                while j < data.len() && data[j] & 0x0f != 0x0f {
                    j += 1;
                }
                (j + 1 - i).min(data.len() - i)
            }
            0..=21 => {
                let len = if b0 == 12 { 2 } else { 1 };
                tokens.push(Token::Operator(&data[i..(i + len).min(data.len())]));
                i += len;
                continue;
            }
            22..=31 | 255 => 1, // reserved; treat as one byte to stay in sync
        };
        tokens.push(Token::Operand(&data[i..(i + len).min(data.len())]));
        i += len;
    }
    tokens
}

fn decode_int(token: &[u8]) -> i64 {
    let Some(&b0) = token.first() else { return 0 };
    match b0 {
        32..=246 => b0 as i64 - 139,
        247..=250 => token
            .get(1)
            .map_or(0, |b| (b0 as i64 - 247) * 256 + *b as i64 + 108),
        251..=254 => token
            .get(1)
            .map_or(0, |b| -((b0 as i64 - 251) * 256 + *b as i64 + 113)),
        28 => token
            .get(1..3)
            .map_or(0, |b| i16::from_be_bytes([b[0], b[1]]) as i64),
        29 => token
            .get(1..5)
            .map_or(0, |b| i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64),
        _ => 0,
    }
}

/// The last pending operand, decoded as a non-negative integer offset.
fn last_operand(operands: &[&[u8]], what: &str) -> Result<usize, BuildError> {
    let last = operands.last().ok_or_else(|| {
        BuildError::UnsupportedFont(format!("CFF: {what} operator without operands"))
    })?;
    match last.first() {
        // 28/29 are integer prefixes; 30 would be a real number
        Some(28 | 29 | 32..=254) => {}
        _ => {
            return Err(BuildError::UnsupportedFont(format!(
                "CFF: {what} operand is not an integer"
            )))
        }
    }
    let value = decode_int(last);
    if value < 0 {
        return Err(BuildError::UnsupportedFont(format!(
            "CFF: {what} offset is negative"
        )));
    }
    Ok(value as usize)
}

struct OffsetTargets {
    charset: u32,
    encoding: Option<u32>,
    charstrings: u32,
    private_offset: Option<u32>,
}

/// Re-emit the Top DICT with the charset (15), Encoding (16), CharStrings
/// (17) and Private-offset (18, second operand) operands replaced by
/// fixed-width 29-encodings. Every other byte passes through unchanged.
fn patch_top_dict(dict: &[u8], targets: &OffsetTargets) -> Vec<u8> {
    let u32_operand = |v: u32| -> Vec<u8> {
        let mut out = vec![29];
        out.extend_from_slice(&v.to_be_bytes());
        out
    };
    let mut out = Vec::with_capacity(dict.len() + 16);
    let mut operands: Vec<&[u8]> = Vec::new();
    for token in tokenize_dict(dict) {
        match token {
            Token::Operand(bytes) => operands.push(bytes),
            Token::Operator(op) => {
                let replacement: Option<u32> = match op {
                    [15] => Some(targets.charset),
                    [16] => targets.encoding,
                    [17] => Some(targets.charstrings),
                    [18] => targets.private_offset,
                    _ => None,
                };
                // everything but the (patched) last operand passes through;
                // Private's size operand is the second-to-last
                for (i, bytes) in operands.iter().enumerate() {
                    let is_last = i + 1 == operands.len();
                    match (replacement, is_last) {
                        (Some(value), true) => out.extend_from_slice(&u32_operand(value)),
                        _ => out.extend_from_slice(bytes),
                    }
                }
                out.extend_from_slice(op);
                operands.clear();
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// INDEX structures
// ---------------------------------------------------------------------------

/// A CFF INDEX viewed in place.
struct IndexView<'a> {
    data: &'a [u8],
    count: usize,
    off_size: usize,
    offsets_at: usize,
    data_at: usize,
}

impl<'a> IndexView<'a> {
    fn new(data: &'a [u8], at: usize) -> Option<Self> {
        if at + 2 > data.len() {
            return None;
        }
        let count = u16::from_be_bytes([data[at], data[at + 1]]) as usize;
        if count == 0 {
            return Some(IndexView {
                data,
                count,
                off_size: 0,
                offsets_at: at + 2,
                data_at: at + 2,
            });
        }
        if at + 3 > data.len() {
            return None;
        }
        let off_size = data[at + 2] as usize;
        if !(1..=4).contains(&off_size) {
            return None;
        }
        let offsets_at = at + 3;
        let data_at = offsets_at + (count + 1) * off_size;
        if data_at > data.len() {
            return None;
        }
        Some(IndexView {
            data,
            count,
            off_size,
            offsets_at,
            data_at,
        })
    }

    fn count(&self) -> usize {
        self.count
    }

    /// 1-based offset value for entry `i` (0-based, so `i` in 0..=count).
    fn offset(&self, i: usize) -> Option<usize> {
        if i > self.count {
            return None;
        }
        let at = self.offsets_at + i * self.off_size;
        if at + self.off_size > self.data.len() {
            return None;
        }
        let mut value = 0usize;
        for b in &self.data[at..at + self.off_size] {
            value = (value << 8) | *b as usize;
        }
        Some(value)
    }

    fn get(&self, i: usize) -> Option<&'a [u8]> {
        if i >= self.count {
            return None;
        }
        let start = self.data_at + self.offset(i)? - 1;
        let end = self.data_at + self.offset(i + 1)? - 1;
        if start > end || end > self.data.len() {
            return None;
        }
        Some(&self.data[start..end])
    }

    /// Total byte size of the INDEX, header to last item.
    fn total_len(&self) -> usize {
        if self.count == 0 {
            return 2;
        }
        self.data_at - self.offsets_at + 3 + self.offset(self.count).unwrap_or(1) - 1
    }
}

/// Byte extent `(start, end)` of the INDEX at `at`.
fn index_extent(data: &[u8], at: usize) -> Option<(usize, usize)> {
    let view = IndexView::new(data, at)?;
    Some((at, at + view.total_len()))
}

/// Minimal offSize able to hold `value` as a 1-based offset.
fn off_size_for(value: usize) -> usize {
    let mut size = 1;
    let mut cap = 0xffusize;
    while value > cap && size < 4 {
        size += 1;
        cap <<= 8;
    }
    size
}

fn push_offset(out: &mut Vec<u8>, value: usize, off_size: usize) {
    let bytes = (value as u32).to_be_bytes();
    out.extend_from_slice(&bytes[4 - off_size..]);
}

/// Re-emit an INDEX with `items` appended: the same items, then the new
/// ones, offsets re-encoded at the offSize the grown table needs.
fn append_index_items(items: &[&[u8]]) -> Vec<u8> {
    let data_len: usize = items.iter().map(|i| i.len()).sum();
    let last_offset = data_len + 1;
    let off_size = off_size_for(last_offset);
    let mut out = Vec::with_capacity(3 + (items.len() + 1) * off_size + data_len);
    out.extend_from_slice(&(items.len() as u16).to_be_bytes());
    out.push(off_size as u8);
    let mut acc = 1usize;
    push_offset(&mut out, acc, off_size);
    for item in items {
        acc += item.len();
        push_offset(&mut out, acc, off_size);
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

// ---------------------------------------------------------------------------
// charset / encoding extents
// ---------------------------------------------------------------------------

/// End offset of a charset covering `num_glyphs` glyphs (including
/// .notdef): the existing coverage is walked to its exact end so the
/// raw bytes can be copied and extended.
fn charset_end(data: &[u8], at: usize, num_glyphs: usize) -> Option<usize> {
    let entries = num_glyphs.checked_sub(1)?;
    let format = *data.get(at)?;
    let mut covered = 0usize;
    let mut pos = at + 1;
    while covered < entries {
        match format {
            0 => {
                let end = pos + 2 * entries;
                return (end <= data.len()).then_some(end);
            }
            1 => {
                let b = data.get(pos..pos + 3)?;
                covered += b[2] as usize + 1;
                pos += 3;
            }
            2 => {
                let b = data.get(pos..pos + 4)?;
                covered += u16::from_be_bytes([b[2], b[3]]) as usize + 1;
                pos += 4;
            }
            _ => return None,
        }
    }
    (covered == entries).then_some(pos)
}

/// Extend raw charset bytes by `add` glyphs. New entries map to SID/CID 0
/// (a name several glyphs may share); range formats split at their nLeft
/// width.
fn extend_charset(raw: &[u8], num_glyphs: usize, add: usize) -> Option<Vec<u8>> {
    let mut out = raw.to_vec();
    match *raw.first()? {
        0 => {
            for _ in 0..add {
                out.extend_from_slice(&0u16.to_be_bytes());
            }
        }
        format @ (1 | 2) => {
            let (max_range, wide): (usize, bool) = if format == 1 {
                (256, false)
            } else {
                (65536, true)
            };
            let mut first = num_glyphs;
            let mut remaining = add;
            while remaining > 0 {
                let take = remaining.min(max_range);
                out.extend_from_slice(&(first as u16).to_be_bytes());
                if wide {
                    out.extend_from_slice(&((take - 1) as u16).to_be_bytes());
                } else {
                    out.push((take - 1) as u8);
                }
                first += take;
                remaining -= take;
            }
        }
        _ => return None,
    }
    Some(out)
}

/// End offset of a custom Encoding structure.
fn encoding_end(data: &[u8], at: usize) -> Option<usize> {
    let format = *data.get(at)?;
    match format {
        0 => {
            let n = *data.get(at + 1)? as usize;
            Some(at + 2 + 2 * n)
        }
        1 => {
            let mut covered = 0usize;
            let mut pos = at + 1;
            while covered < 256 {
                let b = data.get(pos..pos + 3)?;
                covered += b[2] as usize + 1;
                pos += 3;
            }
            (covered == 256).then_some(pos)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use read_fonts::TableProvider;

    #[test]
    fn real_otf_roundtrip_through_read_fonts_parser() {
        // Source Code Pro Regular: name-keyed CFF
        let data = include_bytes!("../tests/fixtures/SourceCodePro-Regular.otf");
        let font = read_fonts::FontRef::new(data).unwrap();
        let table = font
            .data_for_tag(read_fonts::types::Tag::new(b"CFF "))
            .unwrap();
        let bytes = table.as_bytes();

        let layout = parse(bytes).expect("fixture parses");
        let old_glyphs = layout.num_glyphs;
        assert!(old_glyphs > 100, "sanity: {old_glyphs} glyphs");

        let grown = append_empty_glyphs(bytes, &layout, 7).expect("append");
        let layout2 = parse(&grown).expect("grown table re-parses with our parser");
        assert_eq!(layout2.num_glyphs, old_glyphs + 7);

        // independent cross-check: read-fonts' own CFF parser walks the
        // charset and charstrings of the re-laid-out table
        let sfnt = grown_as_sfnt(&grown);
        let font2 = read_fonts::FontRef::new(&sfnt).unwrap();
        let cff = font2.cff().unwrap();
        assert_eq!(cff.top_dicts().count(), 1);
        let charset = cff.charset(0).unwrap();
        assert_eq!(charset.num_glyphs() as usize, old_glyphs + 7);
        // and the postscript name survived the Top DICT surgery
        assert_eq!(cff.name(0).unwrap(), b"SourceCodePro-Regular".as_slice());
    }

    /// Wrap raw CFF bytes back into a minimal sfnt the FontRef can read
    /// (table directory with exactly one entry).
    fn grown_as_sfnt(cff: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + 16 + cff.len());
        out.extend_from_slice(b"OTTO");
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&16u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(b"CFF ");
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(28u32).to_be_bytes());
        out.extend_from_slice(&(cff.len() as u32).to_be_bytes());
        out.extend_from_slice(cff);
        out.extend(std::iter::repeat_n(0u8, (4 - cff.len() % 4) % 4));
        out
    }

    #[test]
    fn dict_tokenizer_splits_operands_and_operators() {
        // 0 (1 byte), 4000 via 28-prefix, charset op 15, then real 3.14
        // via 30-prefix (nibbles 3a 14 1f — the low f terminates), 12 30
        let dict = [139u8, 28, 0x0f, 0xa0, 15, 30, 0x3a, 0x14, 0x1f, 12, 30];
        let tokens = tokenize_dict(&dict);
        let kinds: Vec<bool> = tokens
            .iter()
            .map(|t| matches!(t, Token::Operand(_)))
            .collect();
        assert_eq!(kinds, [true, true, false, true, false]);
        let operand = |i: usize| match tokens[i] {
            Token::Operand(b) => b,
            _ => unreachable!(),
        };
        assert_eq!(decode_int(operand(0)), 0);
        assert_eq!(decode_int(operand(1)), 4000);
        assert_eq!(operand(3).len(), 4, "real number consumes its terminator");
    }

    #[test]
    fn off_size_grows_at_boundaries() {
        assert_eq!(off_size_for(1), 1);
        assert_eq!(off_size_for(0x100), 2);
        assert_eq!(off_size_for(0x10000), 3);
        assert_eq!(off_size_for(0x1000000), 4);
    }

    #[test]
    fn index_roundtrip() {
        // two items, built by hand
        let raw = [0u8, 2, 1, 1, 2, 3, 0xaa, 0xbb];
        let view = IndexView::new(&raw, 0).unwrap();
        assert_eq!(view.count(), 2);
        assert_eq!(view.get(0), Some(&[0xaa][..]));
        assert_eq!(view.get(1), Some(&[0xbb][..]));
        assert_eq!(view.total_len(), raw.len());

        let grown = append_index_items(&[&[0xaa][..], &[0xbb][..], &[ENDCHAR], &[ENDCHAR]]);
        let view2 = IndexView::new(&grown, 0).unwrap();
        assert_eq!(view2.count(), 4);
        assert_eq!(view2.get(0), Some(&[0xaa][..]));
        assert_eq!(view2.get(2), Some(&[ENDCHAR][..]));
        assert_eq!(view2.get(3), Some(&[ENDCHAR][..]));
        assert_eq!(view2.total_len(), grown.len());
    }

    #[test]
    fn index_off_size_upgrade() {
        // 255 bytes of data: appending pushes the last offset past 0x100,
        // so the re-encoded INDEX must grow its offSize
        let big = vec![0xa5u8; 255];
        let grown = append_index_items(&[&big[..], &[ENDCHAR]]);
        assert_eq!(grown[2], 2, "offSize upgraded to 2");
        let view = IndexView::new(&grown, 0).unwrap();
        assert_eq!(view.count(), 2);
        assert_eq!(view.get(0), Some(&big[..]));
        assert_eq!(view.get(1), Some(&[ENDCHAR][..]));
    }

    #[test]
    fn charset_format1_extends_with_one_range() {
        // format 1, one range covering gids 1..2 (first=1, nLeft=1)
        let raw = [1u8, 0, 1, 1];
        let grown = extend_charset(&raw, 3, 2).unwrap();
        assert_eq!(grown, [1, 0, 1, 1, 0, 3, 1]);
        assert_eq!(charset_end(&grown, 0, 5), Some(grown.len()));
    }

    #[test]
    fn charset_format0_appends_sids() {
        let raw = [0u8, 0, 34, 0, 35];
        let grown = extend_charset(&raw, 3, 2).unwrap();
        assert_eq!(grown.len(), 5 + 4);
        assert_eq!(&grown[5..], &[0, 0, 0, 0]);
        assert_eq!(charset_end(&grown, 0, 5), Some(grown.len()));
    }

    #[test]
    fn top_dict_patch_is_length_stable_and_correct() {
        // charset=100 (1-byte operand), CharStrings=4000 (28-prefix),
        // then a non-offset operator group (FontBBox-ish ints)
        let dict = vec![239u8, 15, 28, 0x0f, 0xa0, 17, 155, 156, 5];
        let targets = OffsetTargets {
            charset: 0x1234_5678,
            encoding: None,
            charstrings: 0x10,
            private_offset: None,
        };
        let patched = patch_top_dict(&dict, &targets);
        let again = patch_top_dict(&patched, &targets);
        assert_eq!(patched.len(), again.len(), "patching is length-stable");
        assert_eq!(again, patched, "patching is idempotent");

        // decode back: charset operand now 29-encoded with our value
        let tokens = tokenize_dict(&patched);
        let operand = |i: usize| match tokens[i] {
            Token::Operand(b) => b,
            _ => unreachable!(),
        };
        assert_eq!(operand(0)[0], 29);
        assert_eq!(decode_int(operand(0)), 0x1234_5678);
        assert_eq!(operand(2)[0], 29);
        assert_eq!(decode_int(operand(2)), 0x10);
        assert_eq!(decode_int(operand(4)), 16);
        assert_eq!(decode_int(operand(5)), 17);
    }
}
