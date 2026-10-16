# @chromaglyph/node

napi-rs bindings for [chromaglyph](https://github.com/chromaglyph/chromaglyph):
compile syntax highlighting into an OpenType color font, in-process.

```ts
import { buildFontSafe } from '@chromaglyph/node'

const result = buildFontSafe({
  font: fontBytes,                       // static TTF (Buffer)
  themes: ['dark-plus', 'light-plus'],   // builtin names or .json/.tmTheme paths
  blocks: [                              // batch any number of code blocks into ONE font
    { id: 'block-0', lang: 'rust', text: 'fn main() {}' },
    { id: 'block-1', lang: 'js', text: 'const x = 1' },
  ],
  verify: true,                          // rustybuzz oracle (default on)
  cacheDir: '.cache',                    // optional on-disk cache
})
// → { woff, ttf, css, themes, report, contentHash, cached }
```

- `buildFont` throws on invalid input; `buildFontSafe` returns `null`
  (silent degradation for build pipelines).
- `generic: 'rust' | 'js' | 'ts' | 'python' | 'go' | 'java'` builds a font that
  highlights
  ANY text of that language (keywords, comments, strings, numbers) —
  `blocks` and `semantic` are ignored in that mode.
- Artifacts are cached by content hash (font + theme sources + block texts
  + generic language + options): in-process LRU plus optional on-disk
  cache directory.
- One GSUB rule set serves every theme (CPAL palettes); switching is a
  CSS `@font-palette-values` flip.

Build from source: `pnpm --filter @chromaglyph/node build` (requires Rust).
