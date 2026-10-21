# Chromaglyph

English | [简体中文](README_zh.md)

**Compile syntax highlighting into the font.** Given a monospace font, a set of
themes, and code blocks whose content is known at build time, Chromaglyph
produces a color OpenType font: the code text in the DOM stays
**byte-identical**, and highlighting happens entirely inside the font through
glyph substitution — no `<span>` soup, copy & paste stays clean. Multiple
themes = multiple palettes in one font; light/dark switching is a single CSS
`@font-palette-values` flip with zero re-layout.

**Docs & live demo: <https://chromaglyph.github.io/chromaglyph/>** — the site
is itself a chromaglyph deployment: every code block on it is plain text,
colored by the font. Start with
[Getting Started](https://chromaglyph.github.io/chromaglyph/guide/getting-started).

Inspired by [Cobble / FontWithASyntaxHighlighter](https://github.com/hlotvonen/FontWithASyntaxHighlighter),
which hand-wrote a lexer as a state machine inside the font. Chromaglyph
inverts that: **content is known at build time, so parsing happens at build
time** — only a lookup table ships in the font.

## How it works

```
tokenize + theme → compile → fontbuild → verify
```

- **tokenize + theme** — [syntect](https://github.com/trishume/syntect) parses
  with real TextMate grammars; every character is resolved to a concrete color
  under each theme. All 64 themes of shiki's
  [tm-themes](https://github.com/shikijs/textmate-grammars-themes) collection
  are built in (`chromaglyph themes` lists them), plus your own VS Code
  `.json` / `.tmTheme` files.
- **compile** — each colored run becomes one chained-context substitution rule
  (GSUB ChainContext), anchored by the smallest context that uniquely matches
  it, audited globally. Occurrences that can't be uniquely anchored degrade to
  uncolored plus a warning — **builds never fail**. Rules depend only on color
  slots, so **one GSUB serves every theme**; switching palettes costs nothing.
- **fontbuild** — colored copies are zero-outline glyphs referencing the base
  glyph through COLRv0 (≈ 0 bytes per copy), one CPAL palette per theme. Exact
  mode subsets the font to just the glyphs the blocks use (~8x smaller on real
  pages). Static TTF, name-keyed CFF (OTF), WOFF1/WOFF2 and `.ttc` inputs;
  WOFF1/WOFF2/sfnt outputs.
- **verify** — harfrust shapes the produced font in-process and compares glyph
  ids per character; palettes are read back from CPAL. `build` runs this by
  default.

Two modes:

- **exact** (`--input`): content known at build time; multiple languages share
  one font (disambiguated globally). `--ligatures` preserves the source font's
  ligatures — `!=`, `->` or contextual alternates render exactly as the font
  intends while staying highlighted.
- **generic** (`--generic rust|js|ts|python|go|java|c|cpp|csharp|php|ruby|lua|shell|html|css`):
  no content needed — the font highlights **any** text of that language by
  advancing state purely through glyph substitution. One language per font,
  line-scoped coloring.
- optional `--semantic` (tree-sitter; Rust / JS / TS / Python / Java) overlays
  semantic spans — functions, parameters, types, members — on top of syntax
  colors.

Details, trade-offs and live examples: [How it works](https://chromaglyph.github.io/chromaglyph/guide/how-it-works),
[Languages](https://chromaglyph.github.io/chromaglyph/guide/languages),
[Themes](https://chromaglyph.github.io/chromaglyph/guide/themes).

## Install

- **Prebuilt binaries**: GitHub Releases, 7 targets with `SHA256SUMS`
  (macOS arm64/x64, Linux gnu/musl, Windows x64; musl is fully static).
- **npm**: `npm install -g chromaglyph` — the same CLI, a prebuilt
  binary per platform via `@chromaglyph/cli-*` optional dependencies.
- **cargo**: `cargo install chromaglyph`
- **npm libraries**: `@chromaglyph/node`, plus the `@chromaglyph/remark`,
  `@chromaglyph/markdown-it`, `@chromaglyph/astro`, `@chromaglyph/docusaurus`
  and `@chromaglyph/eleventy` plugins for build-time JS pipelines.

## Quick start

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf \
    --input hello.rs \                    # --lang auto-detects by extension
    --theme dark-plus --theme light-plus \
    --output out.woff --css snippet.css   # + --report report.json for warnings

chromaglyph verify --font out.woff --input hello.rs

# generic mode: no content — the font highlights any text of that language
chromaglyph build --font JetBrainsMono-Regular.ttf --generic rust \
    --output generic.woff --css generic.css
```

The generated CSS turns themes into `@font-palette-values` and follows
`prefers-color-scheme` by default; class-based dark mode (VitePress
`html.dark` etc.) is a two-variable override. Output formats: `.woff` (WOFF1,
default), `.woff2` (~15–30% smaller), `.ttf` (raw sfnt).

From source: `cargo build --release`, then `cargo run -p chromaglyph -- build …`.

See [Getting Started](https://chromaglyph.github.io/chromaglyph/guide/getting-started)
for wiring the font into a page, and
[Integrations](https://chromaglyph.github.io/chromaglyph/guide/integration)
for VitePress, Astro/Starlight, Docusaurus and Eleventy.

## JS plugins

`@chromaglyph/remark` and `@chromaglyph/markdown-it` batch **all fenced code
blocks of a document into one font** (cross-language disambiguation handled
globally), inline the font as a data: URI by default, scope CSS to
`[data-chromaglyph]` so prose is never affected, and silently degrade to
unhighlighted output on any failure. A content-hash cache takes incremental
site builds from 49ms to 5ms; `grouping: 'lang'` opts into one font per
language. Framework adapters build on them: `@chromaglyph/astro`
(Astro / Starlight), `@chromaglyph/docusaurus` (MDX pipeline) and
`@chromaglyph/eleventy` (markdown-it). `@chromaglyph/node` exposes
`buildFont` / `buildFontSafe` plus the cache APIs. See each package's
README and [CHANGELOG.md](CHANGELOG.md) for the full history and
per-version known limitations.

## Known limitations

- **The source font must cover the code's characters.** Missing glyphs (e.g.
  CJK in JetBrains Mono) render through browser fallback and **cannot be
  colored**; use a font with CJK coverage (e.g. Sarasa Mono SC) for Chinese
  comments/strings. Build warnings list the missing characters.
- Variable TrueType sources are pinned to their default instance; CFF2
  (variable-OTF) and CID-keyed CFF sources are rejected with a clear error.
- Generic mode is line-scoped (browsers shape `<pre>` line by line, so state
  dies at newlines) and one language per font.

## Tech

`read-fonts` / `skrifa` / `write-fonts` (fontations) · harfrust (shaping
oracle) · syntect (grammars & themes) · optional tree-sitter (semantic) ·
flate2 (WOFF1) · ttf2woff2 (WOFF2 encode) · wuff (WOFF2 decode).

## License

Apache-2.0. Vendored `themes/*.json` (VS Code theme-defaults, MIT) and the
JetBrains Mono test fixture (OFL) follow their upstream licenses.
