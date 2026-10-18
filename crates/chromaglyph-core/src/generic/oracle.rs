//! The oracle: spec-derived sample texts (curated per language plus a
//! deterministic-random assembly) and the rustybuzz shaper diff that
//! verifies the produced font against the reference lexer.

use std::collections::HashMap;

use crate::verify::VerifyResult;

use super::engine::layout;
use super::langs;
use super::lex::lex;
use super::spec::LanguageSpec;

/// Curated texts per language, plus structural samples derived from the
/// spec and deterministic-random assemblies; the oracle must hold on all.
pub fn sample_texts(spec: &LanguageSpec) -> Vec<String> {
    let mut samples = curated(spec.name);
    let words: Vec<&str> = spec
        .keywords
        .iter()
        .flat_map(|g| g.words.iter().copied())
        .collect();
    // keyword glue and boundaries with the first two words of the spec
    if let (Some(&a), Some(&b)) = (words.first(), words.get(1)) {
        samples.push(format!("{a}{a} {a}x x{a} _{a} {a}_ {b}{a} a {a} b"));
        samples.push(format!("{a}\n{b} {a}{b}\n{a} at_line_start\nplain {a}"));
    }
    // delimiter soup: quote runs and every opener/close adjacency
    let mut soup = String::new();
    for s in spec.strings {
        for _ in 0..3 {
            soup.push(s.quote);
        }
    }
    for sp in spec.spans {
        soup.push_str(sp.open);
        soup.push_str(sp.close);
    }
    samples.push(soup);
    // an unmapped char (emoji) breaks the shaping run: the string dies
    // there and a later quote opens a fresh one
    if let Some(q) = spec.strings.first().map(|s| s.quote) {
        samples.push(format!("a{q}b\u{1F680}c{q}d{q}"));
    }

    // deterministic-random assembly from spec-derived pieces
    let mut pieces: Vec<String> = vec![
        " ".into(),
        "\n".into(),
        "=".into(),
        ";".into(),
        "x".into(),
        "_y1".into(),
        "42".into(),
        "3.14".into(),
        "0x1F".into(),
        ".".into(),
        "e\u{1F680}f".into(),
    ];
    for w in &words {
        pieces.push((*w).to_string());
    }
    for s in spec.strings {
        pieces.push(s.quote.to_string());
        pieces.push(format!("{}ab{}", s.quote, s.quote));
        pieces.push(format!("{}a\nb{}", s.quote, s.quote));
        // escape pairs: `\"` must not close, `\\` must not eat the closer
        pieces.push(format!("{}a\\\"b{}", s.quote, s.quote));
        pieces.push(format!("{}\\\\{}", s.quote, s.quote));
        pieces.push(format!("{}x\\", s.quote)); // dangling escape
                                                // interpolation holes: open/expression/resume, empty, nested-flat
        if let Some((open, close)) = s.interpolation {
            pieces.push(format!("{}a{}b{}c{}", s.quote, open, close, s.quote));
            pieces.push(format!("{}{}{}{}", s.quote, open, close, s.quote));
            pieces.push(format!(
                "{}a{}b{}c{}d{}e{}",
                s.quote, open, open, close, close, s.quote
            ));
        }
    }
    pieces.push("\\".into());
    pieces.push("\\\\".into());
    for sp in spec.spans {
        pieces.push(sp.open.to_string());
        if sp.close.is_empty() {
            pieces.push(format!("{} c\n", sp.open));
        } else {
            pieces.push(sp.close.to_string());
            pieces.push(format!("{} b {}", sp.open, sp.close));
            pieces.push(format!("{}a\nb{}", sp.open, sp.close));
        }
    }
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..400 {
        let n = 4 + (next() as usize % 24);
        let s: String = (0..n)
            .map(|_| pieces[next() as usize % pieces.len()].clone())
            .collect();
        samples.push(s);
    }
    samples
}

fn curated(name: &str) -> Vec<String> {
    match name {
        "c" => langs::c::samples(),
        "cpp" => langs::cpp::samples(),
        "csharp" => langs::csharp::samples(),
        "lua" => langs::lua::samples(),
        "php" => langs::php::samples(),
        "ruby" => langs::ruby::samples(),
        "shell" => langs::shell::samples(),
        "go" => langs::go::samples(),
        "java" => langs::java::samples(),
        "javascript" => langs::javascript::samples(),
        "python" => langs::python::samples(),
        "typescript" => langs::typescript::samples(),
        // rust (and unknown names) keeps the v1.1 curated set
        _ => langs::rust::samples(),
    }
}

/// The generic-mode oracle: shape every sample with the produced font and
/// diff each glyph against the reference lexer's expectation.
pub fn verify_generic(
    ttf: &[u8],
    spec: &LanguageSpec,
    alt_of: &HashMap<(u16, u16), u16>,
    charmap: &dyn Fn(char) -> Option<u16>,
    samples: &[String],
) -> VerifyResult {
    let layout = layout(spec);
    let mut result = VerifyResult::ok();
    let Some(face) = rustybuzz::Face::from_slice(ttf, 0) else {
        result.fail("rustybuzz cannot parse the produced font".into());
        return result;
    };
    for sample in samples {
        let toks = lex(spec, sample, &|ch| charmap(ch).is_some());
        // (byte offset, expected glyph id) per char
        let expected: Vec<(usize, u16)> = sample
            .char_indices()
            .zip(&toks)
            .map(|((byte, ch), &tok)| {
                let gid = charmap(ch).unwrap_or(0);
                let want = match layout.tok_slot(tok) {
                    None => gid,
                    Some(pal) => alt_of.get(&(gid, pal)).copied().unwrap_or(gid),
                };
                (byte, want)
            })
            .collect();

        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(sample);
        let infos = rustybuzz::shape(&face, &[], buffer).glyph_infos().to_vec();

        let mut cells = expected.iter();
        let mut pending = cells.next().copied();
        for info in infos {
            let cluster = info.cluster as usize;
            let Some(&(byte, want)) = pending.as_ref() else {
                result.fail(format!("{sample:?}: extra glyph at cluster {cluster}"));
                break;
            };
            if cluster < byte {
                continue; // merged/hidden cluster artifact
            }
            if cluster > byte {
                result.fail(format!("{sample:?}: char at byte {byte} produced no glyph"));
                break;
            }
            result.glyphs_checked += 1;
            let got = info.glyph_id as u16;
            if got != want {
                let ch: String = sample[byte..].chars().take(1).collect();
                result.fail(format!(
                    "{sample:?} byte {byte} ({ch:?}): got glyph {got}, expected {want}"
                ));
            }
            pending = cells.next().copied();
        }
        if let Some((byte, _)) = pending {
            result.fail(format!("{sample:?}: char at byte {byte} produced no glyph"));
        }
    }
    result
}
