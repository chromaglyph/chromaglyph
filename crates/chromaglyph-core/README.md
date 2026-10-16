# chromaglyph-core

Compile syntax highlighting into an OpenType color font. Pure-function
library: `(font, themes, blocks) → (woff, css, report)`.

- **exact mode**: content is known at build time, so tokenization and scope
  resolution run at build time (syntect grammars + VS Code/TextMate themes);
  the font carries only lookup rules — no lexer state machine
- **slot model**: one distinct per-theme color vector = one palette slot;
  a single GSUB rule set renders every theme, switching is a pure
  `@font-palette-values` flip
- minimal distinguishing contexts with a global false-match audit, 64-glyph
  shaper budget with segment chaining, silent degradation — builds never
  fail on ambiguity
- colored copies are empty glyphs referenced by COLRv0 layers (~zero bytes
  per color); one CPAL palette per theme
- built-in rustybuzz oracle verifies every substituted glyph and every
  palette before the font ships

```rust
use chromaglyph_core::{build, theme::ThemeSpec, spec::Block, BuildInput};

let themes = [ThemeSpec::builtin("dark-plus")?, ThemeSpec::builtin("light-plus")?];
let blocks = [Block { id: "demo".into(), lang: "rust".into(), text: &source }];
let out = build(&BuildInput { font_bytes: &ttf, themes: &themes, blocks: &blocks, run_verify: true })?;
std::fs::write("out.woff", &out.woff)?;
```

CLI: `chromaglyph` (`cargo install chromaglyph`). JS bindings:
`@chromaglyph/node` on npm. Source: <https://github.com/chromaglyph/chromaglyph>.
