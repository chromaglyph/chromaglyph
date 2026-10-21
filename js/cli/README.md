# chromaglyph

**Compile syntax highlighting into the font.** Given a monospace font, a
set of themes, and code blocks whose content is known at build time,
chromaglyph produces a color OpenType font: the code text in the DOM stays
**byte-identical**, and highlighting happens entirely inside the font
through glyph substitution — no `<span>` soup, copy & paste stays clean.
Multiple themes = multiple palettes in one font; light/dark switching is a
single CSS `@font-palette-values` flip with zero re-layout.

This package ships the chromaglyph CLI with a prebuilt binary for every
platform (`@chromaglyph/cli-*` optional dependencies, picked by
os/cpu — the same binaries as the
[GitHub Releases](https://github.com/chromaglyph/chromaglyph/releases);
Node is only needed for the tiny launcher).

```sh
npm install -g chromaglyph

chromaglyph build --font JetBrainsMono-Regular.ttf \
    --input hello.rs \
    --theme dark-plus --theme light-plus \
    --output out.woff --css chromaglyph.css
```

## Also available

- **Rust**: `cargo install chromaglyph`
- **Node API**: `@chromaglyph/node` (`buildFont` / `buildFontSafe`)
- **Doc-site plugins**: `@chromaglyph/markdown-it` (VitePress),
  `@chromaglyph/remark`, `@chromaglyph/astro`, `@chromaglyph/docusaurus`,
  `@chromaglyph/eleventy`

## Links

- Docs & live demo: <https://chromaglyph.github.io/chromaglyph/>
- Getting started:
  <https://chromaglyph.github.io/chromaglyph/guide/getting-started>
- Source: <https://github.com/chromaglyph/chromaglyph>

## License

Apache-2.0
