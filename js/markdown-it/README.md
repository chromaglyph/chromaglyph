# @chromaglyph/markdown-it

markdown-it plugin (VitePress-ready): highlight code blocks by compiling
them into a [chromaglyph](https://github.com/chromaglyph/chromaglyph)
color font. The DOM text stays byte-identical — no `<span>` soup, clean
copy & paste.

```js
import MarkdownIt from 'markdown-it'
import chromaglyph from '@chromaglyph/markdown-it'

const md = new MarkdownIt({ html: true })
md.use(chromaglyph, { font: './JetBrainsMono-Regular.ttf', cacheDir: '.chromaglyph-cache' })

const html = md.render(markdown)
```

The plugin wraps `md.render` — the markdown is parsed once, every
fenced code block batches into a single font (cross-language
disambiguation included), and already-highlighted fences from other
rules chain through untouched. VitePress is supported out of the box:
register inside the markdown config and the existing Shiki/highlight
pipeline keeps working for anything this plugin skips.

- **Zero config**: the font is inlined as a data: URI in an injected
  `<style>` (set `embed: false` plus `outDir` to emit files instead).
- **Silent degradation**: any build failure renders the document
  unhighlighted — your site build never breaks.
- **Cached**: content-hash cache (in-process, plus an optional on-disk
  `cacheDir`).
- `font-family` is scoped to `[data-chromaglyph]` so prose is never
  affected; themes become `@font-palette-values` (dark/light
  auto-switching when two themes are given).
- Introspection: `md.chromaglyph()` returns `{ cached, contentHash, css,
  warnings }` after a render.

## Options

| Option | Default | Description |
|--------|---------|-------------|
| `font` | — | **Required.** Path or buffer of a static TrueType font (e.g. JetBrains Mono). |
| `themes` | `['dark-plus', 'light-plus']` | Builtin theme names or paths to VS Code `.json` / TextMate `.tmTheme` files. |
| `langs` | `null` | Filter fences by language (`['rust', 'ts']`); `null` takes every fenced block. |
| `embed` | `true` | Inline the font as a data: URI; `false` emits files into `outDir`. |
| `outDir` | `'chromaglyph-out'` | Where emitted font/CSS assets go (`embed: false`). |
| `verify` | `true` | Run the rustybuzz oracle over the produced font before shipping it. |
| `cacheDir` | — | Optional on-disk cache directory for incremental site builds. |

## License

Apache-2.0
