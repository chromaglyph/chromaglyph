# chromaglyph

Chromaglyph compiles syntax highlighting into an OpenType **color font**:
given any static TTF, a theme, and code snippets whose content is known at
build time, it emits a subsetted font (COLR + CPAL + contextual GSUB) that
renders those snippets highlighted — with the DOM text staying byte-identical.

**This package is a name reservation.** The npm-distributed CLI (prebuilt
binaries per platform) is under development. Until then, install the Rust CLI:

```sh
cargo install chromaglyph
```

- Source: <https://github.com/chromaglyph/chromaglyph>
- Core library crate: [`chromaglyph-core`](https://crates.io/crates/chromaglyph-core)
