//! The reference lexer: one pass over the text per the spec, one `Tok`
//! per char — the oracle's expectation the produced font must reproduce
//! exactly when shaped (line-scoped; newlines and unmapped chars are
//! hard barriers).

use super::spec::{is_ident, LanguageSpec};

// ---- reference lexer + oracle ---------------------------------------------

/// Reference lexer output per character; the font must reproduce exactly
/// this when shaped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Plain,
    /// group index within `LanguageSpec::keywords`
    Keyword(usize),
    Number,
    /// string kind index within `LanguageSpec::strings`
    StrOpen(usize),
    StrBody(usize),
    StrClose(usize),
    /// `${` opener of an interpolation hole (string-colored); the state
    /// behind it is the interpolation state, not the string state
    InterpOpen(usize),
    /// expression contents of an interpolation hole (uncolored)
    InterpBody(usize),
    /// span kind index within `LanguageSpec::spans`
    SpanOpen(usize),
    SpanBody(usize),
    SpanClose(usize),
    /// markup tag kind index within `LanguageSpec::tags` — the colored
    /// runs and the plain state carriers (the font's uncolored copies
    /// still carry the state class, so the oracle must expect them)
    TagOpen(usize),
    TagName(usize),
    TagAttr(usize),
    TagAttrPlain(usize),
    TagVal(usize),
    TagValPlain(usize),
    TagValStr(usize, usize),
    TagClose(usize),
    /// block kind index within `LanguageSpec::blocks`: a selector word
    /// (colored) and the value regime's plain carriers / exits
    Sel(usize),
    Val(usize),
    ValEnd(usize),
}

/// Highlight `text` per the spec, one [`Tok`] per char, **line-scoped**.
///
/// `mapped` says whether the *output* font has a glyph for a char; it
/// models the shaper's real capabilities. Newlines are hard barriers
/// even when a glyph exists for them: browsers shape each source line as
/// its own run, so no state and no keyword anchor can cross a `\n`.
/// Unmapped chars (emoji, …) break runs the same way. Span openers match
/// longest-first, so `"""` beats `"`.
pub fn lex(spec: &LanguageSpec, text: &str, mapped: &dyn Fn(char) -> bool) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = vec![Tok::Plain; chars.len()];
    // a word boundary glyph: mapped, not an identifier char, not a
    // newline, and not one of the delimiter chars that live in their own
    // classes (quotes, span delimiters, the dot, the escape char)
    let boundary = |c: char| {
        mapped(c)
            && c != '\n'
            && c != '\r'
            && !is_ident(c, spec.ident_extra)
            && !spec.strings.iter().any(|s| s.quote == c)
            && !spec
                .spans
                .iter()
                .any(|s| s.open.contains(c) || s.close.contains(c))
            && c != '.'
            && c != '\\'
            // interpolation opener/closer chars carry their own glyph
            // classes in the engine (exact-sequence `${` matching), so
            // they stop anchoring keywords and numbers there too — the
            // lexer must agree or the oracle diverges (shell `$1` found
            // it; js `{keyword` was latent)
            && !spec.strings.iter().any(|s| match s.interpolation {
                Some((open, close)) => open.contains(c) || close == c,
                None => false,
            })
    };
    let starts = |i: usize, delim: &str| -> bool {
        let cs: Vec<char> = delim.chars().collect();
        chars[i..].starts_with(&cs)
    };
    let all_mapped = |s: &str| s.chars().all(mapped);
    // longest opener first: `"""` must win over `"` at the same position
    let mut span_order: Vec<usize> = (0..spec.spans.len()).collect();
    span_order.sort_by_key(|&j| std::cmp::Reverse(spec.spans[j].open.chars().count()));

    // property-sheet (CSS) regime: None = outside, (kind, false) =
    // selectors, (kind, true) = value regime. Strings and comments
    // reset it — their closing glyphs carry the generic end state in
    // the engine, which no block rule anchors on
    let mut blk: Option<(usize, bool)> = None;
    // index just past a closer that actually closed on this line (a
    // span/string that died at a newline never closed — the engine's
    // end state never existed there, so nothing anchors)
    let mut closed_at: Option<usize> = None;
    let mut i = 0;
    'outer: while i < chars.len() {
        for &j in &span_order {
            let span = &spec.spans[j];
            if !starts(i, span.open) || !all_mapped(span.open) {
                continue;
            }
            let n = span.open.chars().count();
            for tok in out.iter_mut().take(i + n).skip(i) {
                *tok = Tok::SpanOpen(j);
            }
            i += n;
            if span.close.is_empty() {
                while i < chars.len() {
                    let c = chars[i];
                    if c == '\n' || c == '\r' || !mapped(c) {
                        break;
                    }
                    out[i] = Tok::SpanBody(j);
                    i += 1;
                }
            } else {
                while i < chars.len() {
                    if starts(i, span.close) && all_mapped(span.close) {
                        let n = span.close.chars().count();
                        for tok in out.iter_mut().take(i + n).skip(i) {
                            *tok = Tok::SpanClose(j);
                        }
                        i += n;
                        closed_at = Some(i);
                        break;
                    }
                    let c = chars[i];
                    if !mapped(c) || c == '\n' || c == '\r' {
                        break; // barrier or line end: state dies
                    }
                    out[i] = Tok::SpanBody(j);
                    i += 1;
                }
            }
            blk = None;
            if closed_at != Some(i) {
                closed_at = None;
            }
            continue 'outer;
        }
        // markup tags: one inline walk from the opener, mirroring the
        // engine's name → attrs → value machine (delimiters stay plain
        // but carry the state; states die at the newline like all others)
        for (t, tag) in spec.tags.iter().enumerate() {
            if !starts(i, tag.open) || !all_mapped(tag.open) {
                continue;
            }
            let name_start = |c: char| is_ident(c, spec.ident_extra) && !c.is_ascii_digit();
            let mut open_end = i + tag.open.chars().count();
            // the `/` of a closing tag rides on the opener (`</div`)
            if chars.get(open_end) == Some(&'/')
                && mapped('/')
                && chars.get(open_end + 1).copied().is_some_and(name_start)
                && chars.get(open_end + 1).copied().is_some_and(mapped)
            {
                open_end += 1;
            }
            if !chars
                .get(open_end)
                .copied()
                .is_some_and(|c| name_start(c) && mapped(c))
            {
                continue; // `< b`, `<3`, `<!--` (a span): not a tag here
            }
            for tok in out.iter_mut().take(open_end).skip(i) {
                *tok = Tok::TagOpen(t);
            }
            const NAME: u8 = 0;
            const ATTR: u8 = 1;
            const VAL: u8 = 2;
            const VSTR: u8 = 3;
            let mut state = NAME;
            let mut quote = '\0';
            let mut k = open_end;
            while k < chars.len() {
                let c = chars[k];
                if !mapped(c) || c == '\n' || c == '\r' {
                    break; // barrier: the state dies, the char is plain
                }
                let mut closed = false;
                for delim in [tag.self_close, tag.close] {
                    if delim.is_empty() || !starts(k, delim) || !all_mapped(delim) {
                        continue;
                    }
                    let n = delim.chars().count();
                    for tok in out.iter_mut().take(k + n).skip(k) {
                        *tok = Tok::TagClose(t);
                    }
                    k += n;
                    closed = true;
                    break;
                }
                if closed {
                    break;
                }
                match state {
                    NAME => {
                        if is_ident(c, spec.ident_extra) {
                            out[k] = Tok::TagName(t);
                            k += 1;
                        } else {
                            out[k] = Tok::TagAttrPlain(t);
                            state = ATTR;
                            k += 1;
                        }
                    }
                    ATTR => {
                        if c == tag.equals && mapped(tag.equals) {
                            out[k] = Tok::TagValPlain(t);
                            state = VAL;
                            k += 1;
                        } else if c == '<'
                            && chars
                                .get(k + 1)
                                .copied()
                                .is_some_and(|n| name_start(n) && mapped(n))
                        {
                            out[k] = Tok::TagOpen(t); // a new tag opens here
                            state = NAME;
                            k += 1;
                        } else if is_ident(c, spec.ident_extra) {
                            out[k] = Tok::TagAttr(t);
                            k += 1;
                        } else {
                            out[k] = Tok::TagAttrPlain(t);
                            k += 1;
                        }
                    }
                    VAL => {
                        if let Some(qi) = tag
                            .value_quotes
                            .iter()
                            .position(|&qc| qc == c)
                            .filter(|_| mapped(c))
                        {
                            out[k] = Tok::TagValStr(t, qi);
                            quote = c;
                            state = VSTR;
                            k += 1;
                        } else if c == '<'
                            && chars
                                .get(k + 1)
                                .copied()
                                .is_some_and(|n| name_start(n) && mapped(n))
                        {
                            out[k] = Tok::TagOpen(t);
                            state = NAME;
                            k += 1;
                        } else if is_ident(c, spec.ident_extra) {
                            out[k] = Tok::TagVal(t);
                            k += 1;
                        } else {
                            out[k] = Tok::TagAttrPlain(t);
                            state = ATTR;
                            k += 1;
                        }
                    }
                    _ => {
                        // quoted value: only the matching quote closes
                        if c == quote && mapped(c) {
                            out[k] = Tok::TagAttrPlain(t);
                            state = ATTR;
                        } else {
                            let qi = tag
                                .value_quotes
                                .iter()
                                .position(|&qc| qc == quote)
                                .unwrap_or(0);
                            out[k] = Tok::TagValStr(t, qi);
                        }
                        k += 1;
                    }
                }
            }
            i = k;
            continue 'outer;
        }
        for (k, s) in spec.strings.iter().enumerate() {
            if chars[i] != s.quote || !mapped(s.quote) {
                continue;
            }
            out[i] = Tok::StrOpen(k);
            i += 1;
            while i < chars.len() {
                let c = chars[i];
                if c == s.quote && mapped(c) {
                    out[i] = Tok::StrClose(k);
                    i += 1;
                    closed_at = Some(i);
                    break;
                }
                // escape pair: `\` + any mapped non-newline glyph becomes
                // body in one step (mirrors the font's pair rule), so the
                // escaped quote never closes and `\\` keeps the state;
                // literal kinds (Go raw strings) skip this — a backslash
                // there must not stop the quote from closing
                if s.escapes
                    && c == '\\'
                    && mapped(c)
                    && i + 1 < chars.len()
                    && mapped(chars[i + 1])
                    && chars[i + 1] != '\n'
                    && chars[i + 1] != '\r'
                {
                    out[i] = Tok::StrBody(k);
                    out[i + 1] = Tok::StrBody(k);
                    i += 2;
                    continue;
                }
                // interpolation hole: the opener keeps the string color,
                // the expression renders uncolored, the closer resumes the
                // string (mirrors the font's open/propagate/resume rules;
                // flat — the first closer wins, nested openers are body)
                if let Some((open, close)) = s.interpolation {
                    let open_chars: Vec<char> = open.chars().collect();
                    if chars[i..].starts_with(&open_chars)
                        && open_chars.iter().all(|&ch| mapped(ch))
                    {
                        for tok in out.iter_mut().take(i + open_chars.len()).skip(i) {
                            *tok = Tok::InterpOpen(k);
                        }
                        i += open_chars.len();
                        while i < chars.len() {
                            let c = chars[i];
                            if c == close && mapped(c) {
                                out[i] = Tok::StrBody(k); // resume as body
                                i += 1;
                                break;
                            }
                            if !mapped(c) || c == '\n' || c == '\r' {
                                break; // hole dies with the line/barrier
                            }
                            out[i] = Tok::InterpBody(k);
                            i += 1;
                        }
                        continue;
                    }
                }
                if !mapped(c) || c == '\n' || c == '\r' {
                    break; // barrier or line end: state dies
                }
                out[i] = Tok::StrBody(k);
                i += 1;
            }
            blk = None;
            if closed_at != Some(i) {
                closed_at = None;
            }
            continue 'outer;
        }
        if spec.numbers && chars[i].is_ascii_digit() && mapped(chars[i]) {
            let start = i;
            let in_val = matches!(blk, Some((_, true)));
            let closed_anchor = !spec.blocks.is_empty() && closed_at == Some(start);
            if start > 0 && (boundary(chars[start - 1]) || in_val || closed_anchor) {
                while i < chars.len() && is_ident(chars[i], spec.ident_extra) {
                    out[i] = Tok::Number;
                    i += 1;
                }
                // fractional part: a dot continues the number only when
                // a digit follows it (mirrors the font's lookahead rule)
                while i < chars.len() {
                    let continues = is_ident(chars[i], spec.ident_extra)
                        || (chars[i] == '.'
                            && i + 1 < chars.len()
                            && chars[i + 1].is_ascii_digit());
                    if !continues {
                        break;
                    }
                    out[i] = Tok::Number;
                    i += 1;
                }
                continue;
            }
            // a digit with no number anchor is not a number — but it may
            // open a selector word (`.14px` after an anchor): fall
            // through to the ident walk below instead of skipping it
        }
        if is_ident(chars[i], spec.ident_extra) {
            let start = i;
            while i < chars.len() && is_ident(chars[i], spec.ident_extra) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let group = spec
                .keywords
                .iter()
                .position(|g| g.words.contains(&word.as_str()));
            let in_val = matches!(blk, Some((_, true)));
            // a word after a closing delimiter (string quote, span
            // close) anchors only in block languages: there the generic
            // end state is a keyword boundary, elsewhere it is not
            let closed_before = !spec.blocks.is_empty() && closed_at == Some(start);
            let anchor_before = start > 0 && boundary(chars[start - 1]);
            let sel_anchor_before = start > 0
                && (anchor_before
                    || closed_before
                    || spec
                        .blocks
                        .iter()
                        .any(|b| b.anchors.contains(&chars[start - 1])));
            let mut colored = false;
            if let Some(g) = group {
                let la_ok = match spec.keywords[g].followed_by {
                    Some(fc) => chars.get(i) == Some(&fc) && mapped(fc),
                    None => i < chars.len() && boundary(chars[i]),
                };
                if la_ok
                    && word.chars().all(mapped)
                    && start > 0
                    && (anchor_before || closed_before || in_val)
                {
                    for tok in out.iter_mut().take(i).skip(start) {
                        *tok = Tok::Keyword(g);
                    }
                    colored = true;
                }
            }
            if !colored {
                match blk {
                    Some((bi, true)) => {
                        for tok in out.iter_mut().take(i).skip(start) {
                            *tok = Tok::Val(bi);
                        }
                    }
                    Some((bi, false)) => {
                        for tok in out.iter_mut().take(i).skip(start) {
                            *tok = Tok::Sel(bi);
                        }
                    }
                    None if sel_anchor_before && !spec.blocks.is_empty() => {
                        // the open: which block owns it is ambiguous with
                        // several blocks — shipped languages carry one
                        for tok in out.iter_mut().take(i).skip(start) {
                            *tok = Tok::Sel(0);
                        }
                    }
                    None => {}
                }
            }
            continue;
        }
        // property-sheet transitions on the remaining glyphs: the colon
        // enters the value regime after a word, the ends terminate it,
        // everything else rides as an uncolored carrier
        if let Some((bi, val)) = blk {
            let c = chars[i];
            if !mapped(c) || c == '\n' || c == '\r' {
                // barrier: the value regime dies, exactly like the
                // engine's propagation rules (no rule covers a newline)
                blk = None;
                i += 1;
                continue;
            }
            if val {
                if spec.blocks[bi].ends.contains(&c) {
                    out[i] = Tok::ValEnd(bi);
                    blk = None;
                } else {
                    out[i] = Tok::Val(bi);
                }
            } else if c == spec.blocks[bi].colon && mapped(c) {
                out[i] = Tok::Val(bi);
                blk = Some((bi, true));
            }
            // selector regime: no rule fires — the glyph stays plain and
            // the regime survives (it re-anchors on the next boundary)
            i += 1;
            continue;
        }
        if spec.blocks.first().is_some_and(|b| {
            chars[i] == b.colon
                && mapped(chars[i])
                && i > 0
                && is_ident(chars[i - 1], spec.ident_extra)
        }) {
            // a plain word's colon (column-0 property) opens the value
            // regime all the same — the engine anchors on the base class
            blk = Some((0, true));
            out[i] = Tok::Val(0);
            i += 1;
            continue;
        }
        i += 1;
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::generic::{csharp, go, html, java, javascript, python, rust};

    fn toks(s: &str) -> Vec<Tok> {
        // everything mapped except the barrier chars that shape no glyph
        // in a browser (newlines are barriers regardless — see module docs)
        lex(&rust(), s, &|c| c != '\n' && c != '\r')
    }

    #[test]
    fn lex_keywords_and_boundaries() {
        // keyword between punctuation colors; at text start it cannot
        // (no glyph exists for the shaper to backtrack on)
        let t = toks("a let b");
        assert_eq!(&t[2..5], &[Tok::Keyword(0); 3]);
        let t = toks("let x");
        assert_eq!(&t[0..3], &[Tok::Plain; 3], "text-start keyword stays plain");
        // inside an identifier it is never a keyword
        let t = toks("a let_x b");
        assert_eq!(&t[2..6], &[Tok::Plain; 4]);
        // adjacent to delimiter classes it stays plain (those glyphs live
        // outside the not-a-word-char class)
        let t = toks("a let\" b");
        assert_eq!(&t[2..5], &[Tok::Plain; 3]);
    }

    #[test]
    fn lex_line_start_keywords_stay_plain() {
        // every source line is its own shaping run in a browser: a
        // column-0 keyword has no in-run glyph to anchor on — even when
        // the font maps `\n` (pixel-verified in a real browser)
        let t = lex(&rust(), "a\nfn b", &|_| true);
        assert_eq!(&t[2..4], &[Tok::Plain; 2], "line start: no anchor");
        // one space of indentation and the space glyph anchors it
        let t = lex(&rust(), "a\n fn b", &|_| true);
        assert_eq!(&t[3..5], &[Tok::Keyword(0); 2]);
    }

    #[test]
    fn lex_newline_is_a_barrier_even_when_mapped() {
        // states die at the newline; the next line starts a fresh scan
        let t = lex(&rust(), "\"a\nfn let", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(0));
        assert_eq!(t[1], Tok::StrBody(0));
        assert_eq!(t[2], Tok::Plain);
        assert_eq!(&t[3..9], &[Tok::Plain; 6]);
    }

    #[test]
    fn lex_strings_and_comments() {
        let t = toks("a \"b // c\" d");
        assert_eq!(t[2], Tok::StrOpen(0));
        assert_eq!(&t[3..9], &[Tok::StrBody(0); 6]); // `//` inside a string stays string
        assert_eq!(t[9], Tok::StrClose(0));
        let t = toks("// \"q\" x\nfn y");
        assert_eq!(&t[0..2], &[Tok::SpanOpen(0); 2]);
        assert_eq!(&t[2..8], &[Tok::SpanBody(0); 6]); // quote inside a comment stays comment
        assert_eq!(t[8], Tok::Plain); // newline ends the comment
        assert_eq!(
            &t[9..11],
            &[Tok::Plain; 2],
            "line-start keyword stays plain"
        );
    }

    #[test]
    fn lex_block_comments() {
        // block comments are line-scoped: continuation lines start plain
        let t = toks("a /* b\n c */ d");
        assert_eq!(&t[2..4], &[Tok::SpanOpen(1); 2]);
        assert_eq!(&t[4..6], &[Tok::SpanBody(1); 2]); // " b"
        assert_eq!(t[6], Tok::Plain); // the newline itself
        assert_eq!(&t[7..14], &[Tok::Plain; 7], "next line: no state survives");
        // unterminated block comment runs to the line end / barrier
        let t = toks("x /* let fn");
        assert_eq!(&t[2..4], &[Tok::SpanOpen(1); 2]);
        assert_eq!(&t[5..11], &[Tok::SpanBody(1); 6]);
        // `*` alone is body, only `*/` closes
        let t = toks("/* a*b */");
        assert_eq!(t[5], Tok::SpanBody(1));
        assert_eq!(&t[7..9], &[Tok::SpanClose(1); 2]);
    }

    #[test]
    fn lex_numbers() {
        let t = toks("a = 42;");
        assert_eq!(&t[4..6], &[Tok::Number; 2]);
        let t = toks("x = 3.14;");
        assert_eq!(&t[4..8], &[Tok::Number; 4]);
        let t = toks(" 1.max(2)");
        assert_eq!(t[1], Tok::Number);
        assert_eq!(t[2], Tok::Plain, "dot without a following digit stops");
        assert_eq!(t[3], Tok::Plain, "max stays plain");
        assert_eq!(t[7], Tok::Number);
        let t = toks("a1 x42");
        assert_eq!(t[0], Tok::Plain, "digit inside identifier");
        assert_eq!(t[4], Tok::Plain, "digits after ident chars are no number");
        let t = toks(" 0x1F_ff");
        assert_eq!(&t[1..8], &[Tok::Number; 7]);
        let t = lex(&rust(), "\n42", &|_| true);
        assert_eq!(
            &t[1..3],
            &[Tok::Plain; 2],
            "number at line start has no anchor"
        );
        let t = lex(&rust(), "a\n 42", &|_| true);
        assert_eq!(&t[3..5], &[Tok::Number; 2], "indented number colors");
    }

    #[test]
    fn lex_barrier_kills_states() {
        // an unmapped char (here: the emoji) breaks the shaping run: the
        // string dies there and a later quote opens a fresh one
        let t = lex(&rust(), "a\"b\u{1F680}c\"d\"", &|c| c != '\u{1F680}');
        assert_eq!(t[1], Tok::StrOpen(0));
        assert_eq!(t[2], Tok::StrBody(0));
        assert_eq!(t[3], Tok::Plain); // the barrier itself
        assert_eq!(t[4], Tok::Plain); // coloring died with the run break
        assert_eq!(t[5], Tok::StrOpen(0)); // fresh string
        assert_eq!(t[6], Tok::StrBody(0));
    }

    // ---- escapes --------------------------------------------------------

    #[test]
    fn lex_string_escapes_do_not_close() {
        // `\"` is body: the string survives past it and closes at the end
        let t = toks("a = \"x\\\"y\";");
        assert_eq!(t[4], Tok::StrOpen(0));
        assert_eq!(&t[5..9], &[Tok::StrBody(0); 4], "x, \\, \" and y are body");
        assert_eq!(t[9], Tok::StrClose(0), "the real closing quote closes");
        // `\\` is a body pair; the following quote still closes
        let t = toks("\"a\\\\\"");
        assert_eq!(t[0], Tok::StrOpen(0));
        assert_eq!(
            &t[1..4],
            &[Tok::StrBody(0); 3],
            "a and the escaped backslash"
        );
        assert_eq!(t[4], Tok::StrClose(0));
        // escaped quote at the very start, then a real close
        let t = toks("\"\\\"\"");
        assert_eq!(t[0], Tok::StrOpen(0));
        assert_eq!(&t[1..3], &[Tok::StrBody(0); 2]);
        assert_eq!(t[3], Tok::StrClose(0));
        // dangling escape at the line end: pair cannot form, the lone
        // backslash is body and the state dies at the newline
        let t = toks("\"x\\\nlet y");
        assert_eq!(t[2], Tok::StrBody(0), "the lone backslash is body");
        assert_eq!(t[3], Tok::Plain, "the newline");
        assert_eq!(&t[4..7], &[Tok::Plain; 3]);
    }

    #[test]
    fn lex_escapes_in_every_quote_kind() {
        // js: single-quoted with \' and a template with \`
        let spec = javascript();
        let t = lex(&spec, "'it\\'s'", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(1));
        assert_eq!(&t[1..6], &[Tok::StrBody(1); 5]);
        assert_eq!(t[6], Tok::StrClose(1));
        let t = lex(&spec, "`a\\`b`", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(2));
        assert_eq!(&t[1..5], &[Tok::StrBody(2); 4], "escaped backtick is body");
        assert_eq!(t[5], Tok::StrClose(2));
        // python f-string with escapes behaves like any string
        let spec = python();
        let t = lex(&spec, "f\"a\\\"b\"", &|_| true);
        assert_eq!(t[1], Tok::StrOpen(0));
        assert_eq!(&t[2..6], &[Tok::StrBody(0); 4]);
        assert_eq!(t[6], Tok::StrClose(0));
    }

    // ---- JavaScript -----------------------------------------------------

    #[test]
    fn lex_js_quote_kinds_are_independent() {
        let t = lex(&javascript(), "'a\"b'", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(1), "single quote opens kind 1");
        assert_eq!(
            t[2],
            Tok::StrBody(1),
            "double quote inside single-quoted string is body"
        );
        assert_eq!(t[4], Tok::StrClose(1));
        let t = lex(&javascript(), "\"a'b\"", &|_| true);
        assert_eq!(
            t[2],
            Tok::StrBody(0),
            "single quote inside double-quoted string is body"
        );
        // `//` inside any string stays string
        let t = lex(&javascript(), "'a // b'", &|_| true);
        assert_eq!(&t[3..5], &[Tok::StrBody(1); 2]);
    }

    #[test]
    fn lex_js_template_literals_are_line_scoped() {
        // a multi-line template colors its opening line; the continuation
        // line is a fresh run and its closing backtick opens a new,
        // unterminated template
        let t = lex(&javascript(), "`a\nb`", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(2));
        assert_eq!(t[1], Tok::StrBody(2));
        assert_eq!(t[2], Tok::Plain); // newline kills the template
        assert_eq!(t[3], Tok::Plain);
        assert_eq!(t[4], Tok::StrOpen(2), "fresh template on the new line");
        // ordinary quotes die the same way
        let t = lex(&javascript(), "\"a\nb\"", &|_| true);
        assert_eq!(t[2], Tok::Plain, "double-quoted string dies at newline");
        assert_eq!(t[3], Tok::Plain);
        assert_eq!(t[4], Tok::StrOpen(0), "fresh string after the break");
        // interpolation: opener string-colored, expression uncolored,
        // closer resumes the string — like an editor renders templates
        let t = lex(&javascript(), "`x ${y} z`", &|_| true);
        assert_eq!(t[1], Tok::StrBody(2), "x is string body");
        assert_eq!(t[2], Tok::StrBody(2), "space before the hole");
        assert_eq!(&t[3..5], &[Tok::InterpOpen(2); 2], "${{ opens the hole");
        assert_eq!(t[5], Tok::InterpBody(2), "y is uncolored expression");
        assert_eq!(t[6], Tok::StrBody(2), "}} resumes the string");
        assert_eq!(t[7], Tok::StrBody(2));
        assert_eq!(t[9], Tok::StrClose(2));
        // double-quoted strings never open holes: ${} stays string body
        let t = lex(&javascript(), "\"a ${b} c\"", &|_| true);
        assert_eq!(&t[1..9], &[Tok::StrBody(0); 8]);
        // `$` without `{` stays literal string body
        let t = lex(&javascript(), "`a$b`", &|_| true);
        assert_eq!(&t[1..4], &[Tok::StrBody(2); 3]);
        // empty hole `${}` and nested openers (flat: first closer wins)
        let t = lex(&javascript(), "`${}`", &|_| true);
        assert_eq!(&t[1..3], &[Tok::InterpOpen(2); 2]);
        assert_eq!(t[3], Tok::StrBody(2), "empty hole closes immediately");
        assert_eq!(t[4], Tok::StrClose(2));
        // flat nesting: the inner ${{ is plain expression body, the first
        // }} resumes the string, and `c` is string again
        let t = lex(&javascript(), "`${a${b}c`", &|_| true);
        assert_eq!(&t[3..7], &[Tok::InterpBody(2); 4], "a and the inner ${{b");
        assert_eq!(t[7], Tok::StrBody(2), "first }} resumes the string");
        assert_eq!(t[8], Tok::StrBody(2), "c is string again");
        // escaped opener stays literal: \$ then { — no hole
        let t = lex(&javascript(), "`a\\${b}`", &|_| true);
        assert_eq!(t[2], Tok::StrBody(2), "escaped backslash");
        assert_eq!(t[3], Tok::StrBody(2), "the $ after it is literal");
        assert_eq!(t[4], Tok::StrBody(2), "{{ stays body");
    }

    #[test]
    fn lex_js_keyword_groups_and_dollar() {
        let spec = javascript();
        let t = lex(&spec, "a return b", &|_| true);
        assert_eq!(&t[2..8], &[Tok::Keyword(0); 6], "control group");
        let t = lex(&spec, "a function b", &|_| true);
        assert_eq!(&t[2..10], &[Tok::Keyword(1); 8], "storage group");
        let t = lex(&spec, "a true b", &|_| true);
        assert_eq!(&t[2..6], &[Tok::Keyword(2); 4], "constant group");
        // '$' is an identifier char: $return is a name, not a keyword
        let t = lex(&spec, "a $return b", &|_| true);
        assert_eq!(&t[2..9], &[Tok::Plain; 7]);
        let t = lex(&spec, "a return$ b", &|_| true);
        assert_eq!(&t[2..9], &[Tok::Plain; 7]);
        // a keyword may anchor on '$' only when it ends a word — it does
        // not, because '$' is an identifier char
        let t = lex(&spec, "a$ return $b", &|_| true);
        assert_eq!(&t[3..9], &[Tok::Keyword(0); 6]);
    }

    // ---- Python ---------------------------------------------------------

    #[test]
    fn lex_python_triple_quotes_beat_simple() {
        let spec = python();
        // `"""` opens the block string, not an empty `""` pair
        let t = lex(&spec, "\"\"\"x\"\"\"", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(0); 3]);
        assert_eq!(t[3], Tok::SpanBody(0));
        assert_eq!(&t[4..7], &[Tok::SpanClose(0); 3]);
        // still `""` when no third quote follows
        let t = lex(&spec, "\"\"x", &|_| true);
        assert_eq!(t[0], Tok::StrOpen(0));
        assert_eq!(t[1], Tok::StrClose(0));
        assert_eq!(t[2], Tok::Plain);
        // four quotes: triple open + one body char, unterminated
        let t = lex(&spec, "\"\"\"\"", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(0); 3]);
        assert_eq!(t[3], Tok::SpanBody(0));
        // empty triple string (six quotes)
        let t = lex(&spec, "\"\"\"\"\"\"", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(0); 3]);
        assert_eq!(&t[3..6], &[Tok::SpanClose(0); 3]);
        // lone quotes inside a triple string are body
        let t = lex(&spec, "\"\"\"a\"\"b\"\"\"", &|_| true);
        assert_eq!(t[4], Tok::SpanBody(0));
        assert_eq!(t[5], Tok::SpanBody(0));
        assert_eq!(&t[7..10], &[Tok::SpanClose(0); 3]);
    }

    #[test]
    fn lex_python_triple_kinds_and_lines() {
        let spec = python();
        // a multi-line docstring colors its opening line; the second line
        // is a fresh run — its trailing `"""` opens a new (unterminated)
        // triple rather than closing the first
        let t = lex(&spec, "\"\"\"Doc\nstring.\"\"\"", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(0); 3]);
        assert_eq!(&t[3..6], &[Tok::SpanBody(0); 3]); // Doc
        assert_eq!(t[6], Tok::Plain); // the newline
        assert_eq!(&t[7..14], &[Tok::Plain; 7]); // string.
        assert_eq!(&t[14..17], &[Tok::SpanOpen(0); 3]); // fresh, unterminated
                                                        // single-quoted triple is a different kind
        let t = lex(&spec, "'''a\"\"\"b'''", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(1); 3]);
        assert_eq!(
            &t[4..7],
            &[Tok::SpanBody(1); 3],
            "`\"\"\"` inside `'''` is body"
        );
        assert_eq!(&t[8..11], &[Tok::SpanClose(1); 3]);
        // simple strings die at the newline
        let t = lex(&spec, "\"a\nb\"", &|_| true);
        assert_eq!(t[2], Tok::Plain);
        // f-string prefix stays plain, the string still colors
        let t = lex(&spec, "f\"x{y}\"", &|_| true);
        assert_eq!(t[0], Tok::Plain);
        assert_eq!(t[1], Tok::StrOpen(0));
        assert_eq!(&t[2..6], &[Tok::StrBody(0); 4]);
        assert_eq!(t[6], Tok::StrClose(0));
    }

    #[test]
    fn lex_python_hash_comments() {
        let spec = python();
        let t = lex(&spec, "# \"q\" x\ndef y():\n    pass\n", &|_| true);
        assert_eq!(t[0], Tok::SpanOpen(2));
        assert_eq!(&t[1..7], &[Tok::SpanBody(2); 6]);
        assert_eq!(t[7], Tok::Plain, "newline ends the comment");
        assert_eq!(&t[8..11], &[Tok::Plain; 3], "def at line start: no anchor");
        assert_eq!(
            &t[21..25],
            &[Tok::Plain; 4],
            "keyword at line end: no lookahead glyph in its run"
        );
        // indented mid-line keywords color normally
        let t = lex(&spec, "z = 1\n    if x and not y:\n        pass\n", &|_| {
            true
        });
        assert_eq!(&t[10..12], &[Tok::Keyword(0); 2], "if after indentation");
        assert_eq!(&t[15..18], &[Tok::Keyword(3); 3], "and is group 3");
    }

    // ---- Go / Java ------------------------------------------------------

    #[test]
    fn lex_go_raw_strings_have_no_escapes() {
        let spec = go();
        // a backslash before the closing backtick does not suppress it —
        // real Go raw strings have no escape sequences
        // 0:a 1:sp 2:: 3:= 4:sp 5:` 6..15 "ends with " 16:\ 17:` 18:;
        let t = lex(&spec, "a := `ends with \\`;", &|_| true);
        assert_eq!(t[5], Tok::StrOpen(2), "backtick opens the raw string");
        assert_eq!(t[16], Tok::StrBody(2), "the lone backslash is plain body");
        assert_eq!(t[17], Tok::StrClose(2), "the backtick closes even after it");
        assert_eq!(t[18], Tok::Plain);
        // ordinary strings in the same language DO escape
        // 5:" 6:x 7:\ 8:" 9:y 10:"
        let t = lex(&spec, "b := \"x\\\"y\";", &|_| true);
        assert_eq!(t[5], Tok::StrOpen(0));
        assert_eq!(t[8], Tok::StrBody(0), "escaped quote stays body");
        assert_eq!(t[10], Tok::StrClose(0));
    }

    #[test]
    fn lex_go_runes_and_groups() {
        let spec = go();
        // rune literals are the single-quote string kind
        // 5:' 6:\ 7:' 8:' 9:;
        let t = lex(&spec, "c := '\\'';", &|_| true);
        assert_eq!(t[5], Tok::StrOpen(1));
        assert_eq!(&t[6..8], &[Tok::StrBody(1); 2], "escaped rune quote");
        assert_eq!(t[8], Tok::StrClose(1));
        let t = lex(&spec, "x return nil;", &|_| true);
        assert_eq!(&t[2..8], &[Tok::Keyword(0); 6], "control group");
        assert_eq!(&t[9..12], &[Tok::Keyword(2); 3], "constant group");
    }

    #[test]
    fn lex_csharp_verbatim_and_doc() {
        let spec = csharp();
        // the two-char `@"` span beats the plain `"` string kind at the
        // same position (spans match longest-first, strings come after)
        let t = lex(&spec, "a @\"x\" b", &|_| true);
        assert_eq!(
            &t[2..4],
            &[Tok::SpanOpen(3); 2],
            "@\" opens the verbatim span"
        );
        assert_eq!(t[4], Tok::SpanBody(3));
        assert_eq!(t[5], Tok::SpanClose(3));
        // a backslash inside a verbatim string is plain body (no escapes)
        let t = lex(&spec, "@\"a\\b\"", &|_| true);
        assert_eq!(t[3], Tok::SpanBody(3));
        assert_eq!(t[4], Tok::SpanBody(3));
        // doubled quotes close at the first one (documented approximation)
        let t = lex(&spec, "@\"\"\"x\"", &|_| true);
        assert_eq!(&t[0..2], &[Tok::SpanOpen(3); 2]);
        assert_eq!(t[2], Tok::SpanClose(3));
        // `///` (3 chars) beats `//` (2) at the same position
        let t = lex(&spec, "/// doc\n// line", &|_| true);
        assert_eq!(&t[0..3], &[Tok::SpanOpen(0); 3]);
        assert_eq!(&t[8..10], &[Tok::SpanOpen(1); 2]);
        // ordinary strings keep escaping; `$"` interpolates nothing (flat
        // engine): the `$` stays plain and the quote opens a string
        let t = lex(&spec, "s = $\"{a}\";", &|_| true);
        assert_eq!(t[4], Tok::Plain);
        assert_eq!(t[5], Tok::StrOpen(0));
        assert_eq!(&t[6..9], &[Tok::StrBody(0); 3]);
        assert_eq!(t[9], Tok::StrClose(0));
    }

    #[test]
    fn lex_csharp_verbatim_identifiers_stay_plain() {
        let spec = csharp();
        // '@' before a keyword is a verbatim identifier: '@' lives in the
        // span opener, so it is not a boundary glyph and the word cannot
        // anchor its keyword rule
        let t = lex(&spec, "a @return b", &|_| true);
        assert_eq!(&t[2..3], &[Tok::Plain; 1]);
        assert_eq!(&t[3..9], &[Tok::Plain; 6], "@return is an identifier");
        // without the '@', return is a control keyword
        let t = lex(&spec, "a return b", &|_| true);
        assert_eq!(&t[2..8], &[Tok::Keyword(0); 6]);
    }

    #[test]
    fn lex_html_tag_states() {
        let spec = html();
        let t = lex(&spec, "a <div class=\"x\"> b", &|_| true);
        // `a ` plain; `<` opens (plain carrier); div colored as the name
        assert_eq!(&t[0..2], &[Tok::Plain; 2]);
        assert_eq!(t[2], Tok::TagOpen(0));
        assert_eq!(&t[3..6], &[Tok::TagName(0); 3]);
        assert_eq!(t[6], Tok::TagAttrPlain(0), "space: name → attrs");
        assert_eq!(&t[7..12], &[Tok::TagAttr(0); 5], "class");
        assert_eq!(t[12], Tok::TagValPlain(0), "equals");
        assert_eq!(t[13], Tok::TagValStr(0, 0), "opening quote");
        assert_eq!(t[14], Tok::TagValStr(0, 0), "value body");
        assert_eq!(t[15], Tok::TagAttrPlain(0), "closing quote → attrs");
        assert_eq!(t[16], Tok::TagClose(0), "the >");
        assert_eq!(&t[17..19], &[Tok::Plain; 2], "text after the tag");
        // closing tags and self-close: `</div>` `/>`
        let t = lex(&spec, "</div> <br/>", &|_| true);
        assert_eq!(&t[0..2], &[Tok::TagOpen(0); 2], "</ rides the opener");
        assert_eq!(&t[2..5], &[Tok::TagName(0); 3]);
        assert_eq!(t[5], Tok::TagClose(0));
        assert_eq!(t[7], Tok::TagOpen(0));
        assert_eq!(&t[8..10], &[Tok::TagName(0); 2], "br");
        assert_eq!(&t[10..12], &[Tok::TagClose(0); 2], "/> closes both glyphs");
        // an unquoted value colors until the next delimiter
        let t = lex(&spec, "<input type=checkbox>", &|_| true);
        assert_eq!(t[11], Tok::TagValPlain(0), "the equals");
        assert_eq!(&t[12..20], &[Tok::TagVal(0); 8], "checkbox");
        assert_eq!(t[20], Tok::TagClose(0));
        // `<3` `<>` and a spaced `< b` stay plain: no identifier lookahead
        for text in ["<3 x", "<> x", "< b", "a < b"] {
            let t = lex(&spec, text, &|_| true);
            assert!(t.iter().all(|t| *t == Tok::Plain), "{text}: {t:?}");
        }
    }

    #[test]
    fn lex_html_quotes_comments_entities() {
        let spec = html();
        // the OTHER quote inside a quoted value is body; only the
        // matching one closes
        let t = lex(&spec, "<div a='has \"dq\" in'>", &|_| true);
        let open = t
            .iter()
            .position(|t| matches!(t, Tok::TagValStr(0, 1)))
            .unwrap();
        assert_eq!(t[open], Tok::TagValStr(0, 1), "opening single quote");
        // every `"` between the single quotes is value body of quote-kind 1
        let body: Vec<Tok> = t[open + 1..open + 12].to_vec();
        assert!(
            body.iter().all(|t| matches!(t, Tok::TagValStr(0, 1))),
            "{body:?}"
        );
        // comments swallow tags; entities span `&` to `;`
        let t = lex(&spec, "<!-- <div class=\"x\"> -->", &|_| true);
        assert_eq!(&t[0..4], &[Tok::SpanOpen(0); 4]);
        assert_eq!(&t[21..24], &[Tok::SpanClose(0); 3]);
        let t = lex(&spec, "a &amp; b", &|_| true);
        assert_eq!(t[2], Tok::SpanOpen(1));
        assert_eq!(&t[3..6], &[Tok::SpanBody(1); 3]);
        assert_eq!(t[6], Tok::SpanClose(1));
        // an unclosed tag dies at the newline: continuation is plain
        let t = lex(&spec, "<div class=\"x\"", &|_| true);
        assert!(t.iter().all(|t| *t != Tok::Plain));
        let t = lex(&spec, "<div\nclass=\"x\"", &|_| true);
        assert!(
            t[5..].iter().all(|t| *t == Tok::Plain),
            "no state crosses the newline"
        );
    }

    #[test]
    fn lex_java_char_literals() {
        let spec = java();
        // 0:v 2..5:char 7:q 9:= 11:' 12:\ 13:' 14:' 15:;
        let t = lex(&spec, "v char q = '\\'';", &|_| true);
        assert_eq!(&t[2..6], &[Tok::Keyword(1); 4], "char is storage.type");
        assert_eq!(t[11], Tok::StrOpen(1));
        assert_eq!(&t[12..14], &[Tok::StrBody(1); 2], "escaped quote in char");
        assert_eq!(t[14], Tok::StrClose(1));
        let t = lex(&spec, "x return null;", &|_| true);
        assert_eq!(&t[2..8], &[Tok::Keyword(0); 6], "control group");
        assert_eq!(&t[9..13], &[Tok::Keyword(4); 4], "constant group");
    }
}
