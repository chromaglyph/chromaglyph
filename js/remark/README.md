# @chromaglyph/remark

remark plugin: highlight code blocks by compiling them into a
[chromaglyph](https://github.com/chromaglyph/chromaglyph) color font.
The DOM text stays byte-identical — no `<span>` soup, clean copy & paste.

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

- **One font per document**: every fenced code block batches into a
  single font, with cross-language disambiguation handled globally.
- **Zero config**: the font is inlined as a data: URI in an injected
  `<style>` (set `embed: false` plus `outDir` to emit files instead).
- **Silent degradation**: any build failure renders the document
  unhighlighted — your site build never breaks.
- **Cached**: content-hash cache (in-process, plus an optional on-disk
  `cacheDir`).
- `font-family` is scoped to `[data-chromaglyph]` so prose is never
  affected; themes become `@font-palette-values` (dark/light
  auto-switching when two themes are given).

Inspect `file.data.chromaglyph` for `{ cached, contentHash, css, warnings }`.

## Options

| Option | Default | Description |
|--------|---------|-------------|
| `font` | — | **Required.** Path or buffer of a static TrueType font (e.g. JetBrains Mono). |
| `themes` | `['dark-plus', 'light-plus']` | Builtin theme names or paths to VS Code `.json` / TextMate `.tmTheme` files. |
| `langs` | `null` | Filter fences by language (`['rust', 'ts']`); `null` takes every fenced block. |
| `embed` | `true` | Inline the font as a data: URI; `false` emits files into `outDir`. |
| `outDir` | `'chromaglyph-out'` | Where emitted font/CSS assets go (`embed: false`). |
| `verify` | `true` | Run the rustybuzz oracle over the produced font before shipping it. |
| `semantic` | `false` | Overlay tree-sitter semantic colors (parameters, variables-as-scoped, types, calls) on top of the syntactic highlighting — rust/js/ts/python only; other languages silently stay syntactic. |
| `cacheDir` | — | Optional on-disk cache directory for incremental site builds. |

## License

Apache-2.0
