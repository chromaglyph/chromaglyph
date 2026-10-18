# Chromaglyph

English | [简体中文](README_zh.md)

**Compile syntax highlighting into the font.** Given a monospace font, a set of
themes, and code blocks whose content is known at build time, Chromaglyph
produces a color OpenType font: the code text in the DOM stays
**byte-identical**, and highlighting happens entirely inside the font through
glyph substitution — no `<span>` soup, copy & paste stays clean. Multiple
themes = multiple palettes in one font; light/dark switching is a single CSS
`@font-palette-values` flip with zero re-layout.

Inspired by [Cobble / FontWithASyntaxHighlighter](https://github.com/ssssota/FontWithASyntaxHighlighter),
which hand-wrote a lexer as a state machine inside the font (706 `calt`
rules advancing state character by character). Chromaglyph inverts that:
**content is known at build time, so parsing happens at build time** — only
a lookup table ships in the font.

## How it works

```
tokenize + theme → compile → fontbuild → verify
```

- **tokenize + theme** — [syntect](https://github.com/trishume/syntect)
  parses with real TextMate grammars, then every character is resolved to a
  concrete color under each theme. Themes: VS Code `.json` (JSONC +
  `include` chains) and `.tmTheme`; `dark-plus` / `light-plus` built in.
- **compile** — each colored run becomes one chained-context substitution
  rule (GSUB ChainContext), using the smallest context that uniquely
  anchors it, audited globally for false matches across all blocks.
  Occurrences that can't be uniquely anchored degrade to uncolored plus a
  warning — **builds never fail**. Rules depend only on color slots, so
  **one GSUB serves every theme**; switching palettes costs nothing.
- **fontbuild** — source tables are preserved byte-for-byte where possible;
  colored copies are zero-outline glyphs referencing the base glyph through
  COLRv0 (≈ 0 bytes per copy), with one CPAL palette per theme. The
  source font's own GSUB/GPOS/GDEF and AAT tables are replaced to avoid
  interference. Sources: static TrueType (TTF) and name-keyed CFF (OTF) —
  the latter via byte-level CharStrings surgery — exact mode subsets CFF
  like TrueType (dropped glyphs' charstrings and name strings go away;
  hinting subroutines ride along); WOFF1/WOFF2 containers and `.ttc`
  collections (`--font-index`) are unwrapped on input; variable TrueType
  fonts are pinned to their default instance (the glyf outlines *are* the
  default, so the variation tables are simply dropped). CFF2
  (variable-OTF) outlines and CID-keyed CFF are rejected with a clear
  error.
- **verify** — rustybuzz shapes the produced font in-process and compares
  glyph ids per character (shaping is theme-independent, so one pass
  suffices); palettes are read back from CPAL. `build` runs this by
  default.

Two modes:

- **exact** (`--input`): content known at build time. Multiple languages
  share one font (disambiguated globally), and since 0.2.2 the font is
  **subset to just the glyphs the blocks use** (~8x smaller on real pages).
  Since 0.2.3, `--ligatures` preserves the source font's ligatures: the
  build shapes each block with the source font and re-materializes every
  substitution as a rule — `!=`, `->` or JetBrains Mono's contextual
  alternates render exactly as the font intends while staying highlighted.
  Ligature sites spanning multiple colors degrade to plain per-char text
  (colors win); one glyph can only carry one color.
- **generic** (`--generic rust|js|ts|python|go|java|c|cpp|csharp|php|ruby|lua|shell`): no content needed —
  the font highlights **any** text of that language (keywords, comments,
  strings, numbers) by advancing state purely through glyph substitution,
  like the original project's FSM but synthesized from a language spec in
  ~100 rules. One language per font. Coloring is line-scoped: browsers
  shape `<pre>` line by line, so state dies at newlines (multi-line block
  comments / template strings color their starting line only).
- optional `--semantic` (tree-sitter; Rust / JS / TS / Python / Java)
  overlays semantic spans — functions, parameters, types, members — on top
  of syntax colors, like an editor with a language server.

## Install

- **Prebuilt binaries**: GitHub Releases, 7 targets with `SHA256SUMS`
  (macOS arm64/x64, Linux gnu/musl, Windows x64; musl is fully static).
  macOS builds are ad-hoc signed — first run may need
  `xattr -d com.apple.quarantine <binary>`.
- **cargo**: `cargo install chromaglyph`
- **npm**: `@chromaglyph/node`, plus the `@chromaglyph/remark` and
  `@chromaglyph/markdown-it` plugins.

## CLI

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf \
    --input examples/hello.rs \             # --lang auto detects by extension
    --theme dark-plus --theme light-plus \  # or my-theme.json / my.tmTheme, repeatable
    --ligatures \                           # keep the font's != -> ligatures, highlighted
    --output out.woff --css snippet.css --report report.json

chromaglyph verify --font out.woff --input examples/hello.rs

# generic mode: no content — the font highlights any text of that language
chromaglyph build --font JetBrainsMono-Regular.ttf --generic rust \
    --output generic.woff --css generic.css
```

Generated CSS (excerpt) — themes become `@font-palette-values`, dark/light
follows `prefers-color-scheme` by default:

```css
@font-face { font-family: 'chromaglyph'; src: url('out.woff') format('woff'); }
@font-palette-values --chromaglyph-dark-plus  { font-family: chromaglyph; base-palette: 0; }
@font-palette-values --chromaglyph-light-plus { font-family: chromaglyph; base-palette: 1; }
:root { /* hash-free aliases; class-toggle dark mode below */
  --chromaglyph-dark-palette: --chromaglyph-dark-plus;
  --chromaglyph-light-palette: --chromaglyph-light-plus;
}
pre, code {
  font-family: chromaglyph, monospace;
  font-palette: var(--chromaglyph-palette, var(--chromaglyph-dark-palette));
  background: var(--chromaglyph-bg, #1e1e1e);
}
@media (prefers-color-scheme: light) {
  pre, code { font-palette: var(--chromaglyph-palette, var(--chromaglyph-light-palette));
              background: var(--chromaglyph-bg, #ffffff); }
}
/* sites that toggle a class (VitePress html.dark etc.) take over BOTH
   branches in one place:
   :root     { --chromaglyph-palette: var(--chromaglyph-light-palette);
               --chromaglyph-bg: <your code-block bg variable>; }
   html.dark { --chromaglyph-palette: var(--chromaglyph-dark-palette); } */
```

`.ttf` writes raw sfnt; `.woff` (default) writes WOFF1 — pure Rust has no
mature WOFF2 encoder yet, and every browser supports WOFF1.

From source:

```sh
cargo build --release
cargo run -p chromaglyph -- build \
    --font crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf \
    --input examples/hello.rs \
    --output target/demo/chromaglyph-demo.woff --css target/demo/chromaglyph-demo.css
python3 -m http.server -d target/demo 8143   # open index.html, toggle dark/light
```

## JS plugins

`@chromaglyph/remark` and `@chromaglyph/markdown-it` (VitePress) batch
**all fenced code blocks of a document into one font** (cross-language
disambiguation handled globally), inline the font as a data: URI by default,
scope CSS to `[data-chromaglyph]` so prose is never affected, and silently
degrade to unhighlighted output on any failure. A content-hash cache
(in-process LRU + optional on-disk) takes incremental site builds from
49ms to 5ms. Since 0.2.2, `grouping: 'lang'` opts into one font per
language instead.

```js
import { unified } from 'unified'
import remarkParse from 'remark-parse'
import remarkRehype from 'remark-rehype'
import rehypeStringify from 'rehype-stringify'
import remarkChromaglyph from '@chromaglyph/remark'

const file = await unified()
  .use(remarkParse)
  .use(remarkChromaglyph, { font: './JetBrainsMono-Regular.ttf' })
  .use(remarkRehype, { allowDangerousHtml: true })
  .use(rehypeStringify, { allowDangerousHtml: true })
  .process(markdown)
```

`@chromaglyph/node` exposes `buildFont` / `buildFontSafe` (returns `null`
instead of throwing) plus the cache APIs. See each package's README for
details, and [CHANGELOG.md](CHANGELOG.md) for the full history and
per-version known limitations.

## Known limitations

- **The source font must cover the code's characters.** Glyphs missing
  from it (e.g. CJK in JetBrains Mono) render through browser fallback and
  **cannot be colored** — coloring happens in this font's substitutions;
  surrounding ASCII still colors. Build warnings list the missing
  characters; use a font with CJK coverage (e.g. Sarasa Mono SC) for
  Chinese comments/strings.
- Inputs: static TrueType (TTF), name-keyed CFF (OTF), WOFF1, WOFF2 and
  `.ttc` collections (`--font-index` picks the member); variable TrueType
  fonts are pinned to their default instance. CFF2 (variable-OTF) and
  CID-keyed CFF sources are rejected. Output is WOFF1 (or raw sfnt) — no
  pure-Rust WOFF2 encoder exists yet.
- Generic mode is line-scoped (see above) and one language per font —
  merging languages would color `type` in a Rust block via TS rules.
- Generic mode keywords/numbers need a preceding in-line boundary
  glyph, so a keyword at the very start of a line stays plain (a
  designed limit of the in-font lexer; everything after it colors
  normally).

## Size comparison (`examples/hello.rs`, 114 chars, two themes)

| Table | Chromaglyph | FontWithASyntaxHighlighter |
|-------|-------------|-----------------------------|
| GSUB  | 1.2 KB (exact mode, anchoring rules only) | 39 KB (full FSM) |
| COLR  | 0.4 KB (empty outlines + layer refs) | 12.7 KB (copied outlines) |
| CPAL  | 40 B (2 palettes × 6 slots) | 50 B (1 palette) |
| Rules | 9 | 706 |

Self stress test (`compile.rs` itself, 31,535 chars, two themes): 622
rules, 7 slots, 241 colored copies, built in 0.14s. The global false-match
audit uses a suffix-array index over glyph sequences — O(n log n) queries,
0.54s on 126k chars of input, byte-identical output to the naive scan.

## Tech

`read-fonts` / `skrifa` / `write-fonts` (fontations) · rustybuzz (shaping
oracle) · syntect (grammars & themes) · optional tree-sitter (semantic) ·
flate2 (WOFF1).

## License

Apache-2.0. Vendored `themes/*.json` (VS Code theme-defaults, MIT) and the
JetBrains Mono test fixture (OFL) follow their upstream licenses.
