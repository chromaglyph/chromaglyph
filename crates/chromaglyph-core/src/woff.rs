//! WOFF wrapping of the produced sfnt.
//!
//! Two containers, one function each: WOFF1 (per-table zlib) and WOFF2
//! (single Brotli stream, via the pure-Rust `ttf2woff2` encoder). The
//! glyf/loca table transformation is deliberately left off — upstream
//! reports reconstructed glyf can exceed the short-loca limit without
//! switching formats, which Chromium rejects (fonttools still reads it);
//! the untransformed path costs only a few percent and is what every
//! decoder agrees on. WOFF2 also carries only TrueType-flavored fonts in
//! that encoder, so CFF-flavored output (OTF sources) wraps WOFF1 —
//! decided in [`crate::build`], not here.

use std::io::{Read, Write};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use read_fonts::types::Tag;
use read_fonts::{FontRef, TableProvider};

use crate::error::BuildError;

const WOFF_SIGNATURE: &[u8; 4] = b"wOFF";

/// Wrap a built sfnt (ttf) into WOFF1.
pub fn wrap(ttf: &[u8]) -> Result<Vec<u8>, BuildError> {
    let font =
        FontRef::new(ttf).map_err(|e| BuildError::Woff(format!("cannot parse sfnt: {e}")))?;
    let records = font.table_directory().table_records().to_vec();

    struct Entry {
        tag: [u8; 4],
        data: Vec<u8>, // possibly compressed
        orig_length: u32,
        orig_checksum: u32,
    }
    let mut entries = Vec::with_capacity(records.len());
    for record in &records {
        let tag = record.tag();
        let raw = font
            .data_for_tag(tag)
            .ok_or_else(|| BuildError::Woff(format!("table {tag} unreadable")))?
            .as_bytes();
        let compressed = zlib(raw);
        let data = if compressed.len() < raw.len() {
            compressed
        } else {
            raw.to_vec()
        };
        entries.push(Entry {
            tag: tag.to_be_bytes(),
            data,
            orig_length: raw.len() as u32,
            orig_checksum: record.checksum(),
        });
    }

    let header_len = 44 + 20 * entries.len();
    let mut offset = header_len as u32;
    let mut body: Vec<u8> = Vec::new();
    let mut directory: Vec<u8> = Vec::new();
    for entry in &entries {
        let this_offset = offset;
        body.extend_from_slice(&entry.data);
        let padding = (4 - entry.data.len() % 4) % 4;
        body.extend(std::iter::repeat_n(0u8, padding));
        offset += entry.data.len() as u32 + padding as u32;
        directory.extend_from_slice(&entry.tag);
        directory.extend_from_slice(&this_offset.to_be_bytes());
        directory.extend_from_slice(&(entry.data.len() as u32).to_be_bytes());
        directory.extend_from_slice(&entry.orig_length.to_be_bytes());
        directory.extend_from_slice(&entry.orig_checksum.to_be_bytes());
    }
    let total = offset;

    let mut woff = Vec::with_capacity(total as usize);
    woff.extend_from_slice(WOFF_SIGNATURE);
    // flavor mirrors the sfnt version: TrueType or OTTO for CFF outlines
    woff.extend_from_slice(&ttf[0..4]);
    woff.extend_from_slice(&total.to_be_bytes());
    woff.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    woff.extend_from_slice(&0u16.to_be_bytes()); // reserved
    woff.extend_from_slice(&(ttf.len() as u32).to_be_bytes()); // totalSfntSize
    woff.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
    woff.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
    woff.extend_from_slice(&0u32.to_be_bytes()); // metaOffset
    woff.extend_from_slice(&0u32.to_be_bytes()); // metaLength
    woff.extend_from_slice(&0u32.to_be_bytes()); // metaOrigLength
    woff.extend_from_slice(&0u32.to_be_bytes()); // privOffset
    woff.extend_from_slice(&0u32.to_be_bytes()); // privLength
    debug_assert_eq!(woff.len(), 44);
    woff.extend_from_slice(&directory);
    debug_assert_eq!(woff.len(), header_len);
    woff.extend_from_slice(&body);
    debug_assert_eq!(woff.len(), total as usize);
    Ok(woff)
}

/// Wrap a built sfnt (ttf) into WOFF2 (single Brotli stream, tables
/// untransformed). TrueType flavor only — the caller falls back to WOFF1
/// for CFF-flavored output.
pub fn wrap_woff2(ttf: &[u8]) -> Result<Vec<u8>, BuildError> {
    ttf2woff2::encode(ttf, ttf2woff2::BrotliQuality::default())
        .map_err(|e| BuildError::Woff(format!("woff2 encode: {e}")))
}

/// Unwrap a WOFF1 back into a standalone sfnt (for `chromaglyph verify`).
pub fn unwrap(woff: &[u8]) -> Result<Vec<u8>, BuildError> {
    if woff.len() < 44 || &woff[0..4] != WOFF_SIGNATURE {
        return Err(BuildError::Woff("not a WOFF file".into()));
    }
    let be32 = |at: usize| u32::from_be_bytes(woff[at..at + 4].try_into().unwrap());
    let be16 = |at: usize| u16::from_be_bytes(woff[at..at + 2].try_into().unwrap());
    let num_tables = be16(12) as usize;
    if woff.len() < 44 + 20 * num_tables {
        return Err(BuildError::Woff("truncated WOFF directory".into()));
    }

    struct Table {
        tag: [u8; 4],
        orig_length: u32,
        data: Vec<u8>,
    }
    let mut tables = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let at = 44 + 20 * i;
        let tag: [u8; 4] = woff[at..at + 4].try_into().unwrap();
        let offset = be32(at + 4) as usize;
        let comp_length = be32(at + 8) as usize;
        let orig_length = be32(at + 12) as usize;
        let end = offset
            .checked_add(comp_length)
            .ok_or_else(|| BuildError::Woff("bad table extent".into()))?;
        if end > woff.len() {
            return Err(BuildError::Woff(format!(
                "table {} out of bounds",
                Tag::new(&tag)
            )));
        }
        let chunk = &woff[offset..end];
        let data = if comp_length < orig_length {
            let mut z = flate2::read::ZlibDecoder::new(chunk);
            let mut out = Vec::with_capacity(orig_length);
            z.read_to_end(&mut out)
                .map_err(|e| BuildError::Woff(format!("zlib: {e}")))?;
            out
        } else {
            chunk.to_vec()
        };
        if data.len() != orig_length {
            return Err(BuildError::Woff(format!(
                "table {}: inflated to {} bytes, header says {}",
                Tag::new(&tag),
                data.len(),
                orig_length
            )));
        }
        tables.push(Table {
            tag,
            orig_length: orig_length as u32,
            data,
        });
    }

    // assemble sfnt: 12-byte header + 16-byte records + 4-aligned table data
    let n = tables.len();
    let entry_selector = (31 - (n as u32).leading_zeros()) as u16; // floor(log2 n)
    let search_range: u16 = 16 * (1 << entry_selector);
    let mut ttf: Vec<u8> = Vec::with_capacity(
        12 + 16 * n
            + tables
                .iter()
                .map(|t| t.orig_length as usize + 3)
                .sum::<usize>(),
    );
    ttf.extend_from_slice(&woff[4..8]); // flavor: TrueType or OTTO
    ttf.extend_from_slice(&(n as u16).to_be_bytes());
    ttf.extend_from_slice(&search_range.to_be_bytes());
    ttf.extend_from_slice(&entry_selector.to_be_bytes());
    ttf.extend_from_slice(&((n as u16 * 16).wrapping_sub(search_range)).to_be_bytes());
    let mut offset = 12 + 16 * n;
    let mut head_offset = None;
    for table in &tables {
        let checksum = checksum(&table.data);
        ttf.extend_from_slice(&table.tag);
        ttf.extend_from_slice(&checksum.to_be_bytes());
        ttf.extend_from_slice(&(offset as u32).to_be_bytes());
        ttf.extend_from_slice(&table.orig_length.to_be_bytes());
        if &table.tag == b"head" {
            head_offset = Some(offset);
        }
        offset += table.orig_length as usize;
        offset += (4 - table.orig_length as usize % 4) % 4;
    }
    for table in &tables {
        ttf.extend_from_slice(&table.data);
        ttf.extend(std::iter::repeat_n(0u8, (4 - table.data.len() % 4) % 4));
    }
    // fix head.checkSumAdjustment
    if let Some(head_at) = head_offset {
        let adj_at = head_at + 8;
        ttf[adj_at..adj_at + 4].copy_from_slice(&[0; 4]);
        let sum = checksum(&ttf);
        let adj = 0xB1B0_AFBAu32.wrapping_sub(sum);
        ttf[adj_at..adj_at + 4].copy_from_slice(&adj.to_be_bytes());
    }
    Ok(ttf)
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
    encoder.write_all(data).expect("in-memory zlib cannot fail");
    encoder.finish().expect("in-memory zlib cannot fail")
}

/// OpenType table checksum: sum of big-endian u32s over 4-byte-padded data.
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u64;
    let mut i = 0;
    while i + 4 <= data.len() {
        sum += u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as u64;
        i += 4;
    }
    if i < data.len() {
        let mut last = [0u8; 4];
        last[..data.len() - i].copy_from_slice(&data[i..]);
        sum += u32::from_be_bytes(last) as u64;
    }
    (sum & 0xFFFF_FFFF) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_spec_example() {
        // "abcd" -> 0x61626364
        assert_eq!(checksum(b"abcd"), 0x6162_6364);
        // "abcdef" -> 0x61626364 + 0x65660000
        assert_eq!(
            checksum(b"abcdef"),
            0x6162_6364u32.wrapping_add(0x6566_0000)
        );
    }

    #[test]
    fn zlib_roundtrip() {
        let data = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbb";
        let compressed = zlib(data);
        let mut z = flate2::read::ZlibDecoder::new(&compressed[..]);
        let mut out = Vec::new();
        z.read_to_end(&mut out).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn woff2_wrap_decodes_back_losslessly() {
        // any parseable sfnt exercises the wrap; wuff (the decoder the input
        // path already uses, verified byte-identical to google/woff2) is the
        // oracle: per-table bytes must survive the container unchanged
        let ttf = std::fs::read("tests/fixtures/JetBrainsMono-Regular.ttf").unwrap();
        let woff2 = wrap_woff2(&ttf).unwrap();
        assert_eq!(&woff2[0..4], b"wOF2");
        let decoded = wuff::decompress_woff2(&woff2).unwrap();
        let a = FontRef::new(&ttf).unwrap();
        let b = FontRef::new(&decoded).unwrap();
        let tags: Vec<Tag> = a
            .table_directory()
            .table_records()
            .iter()
            .map(|r| r.tag())
            .collect();
        assert_eq!(tags.len(), b.table_directory().table_records().len());
        for tag in tags {
            let (mut x, mut y) = (
                a.data_for_tag(tag).unwrap().as_bytes().to_vec(),
                b.data_for_tag(tag).unwrap().as_bytes().to_vec(),
            );
            // head.checkSumAdjustment is a checksum over the whole font, so
            // decoders recompute it on reconstruction — the one field that
            // legitimately differs. Mask it on both sides.
            if tag == Tag::new(b"head") && x.len() >= 12 {
                x[8..12].fill(0);
                y[8..12].fill(0);
            }
            assert_eq!(x, y, "table {tag} changed through the woff2 roundtrip");
        }
    }

    #[test]
    fn woff2_is_smaller_than_woff1() {
        let ttf = std::fs::read("tests/fixtures/JetBrainsMono-Regular.ttf").unwrap();
        let w1 = wrap(&ttf).unwrap();
        let w2 = wrap_woff2(&ttf).unwrap();
        // brotli over one stream beats per-table zlib; an encoder silently
        // falling back to stored tables would not even undercut the raw sfnt
        assert!(
            w2.len() < w1.len(),
            "woff2 {} vs woff1 {}",
            w2.len(),
            w1.len()
        );
        assert!(
            w2.len() < ttf.len(),
            "woff2 {} vs ttf {}",
            w2.len(),
            ttf.len()
        );
    }
}
