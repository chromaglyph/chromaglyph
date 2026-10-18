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
  `cacheDir`; the disk cache is skipped when `NODE_ENV=development` so a
  dev server's HMR edits cannot grow it without bound).
- `font-family` is scoped to `[data-chromaglyph]` so prose is never
  affected; themes become `@font-palette-values` (dark/light
  auto-switching when two themes are given).
- **Dark mode**: the generated CSS follows `prefers-color-scheme` by
  default; class-toggle sites override it with two CSS variables
  (`--chromaglyph-palette`, `--chromaglyph-bg` — the stylesheet defines
  hash-free `--chromaglyph-dark/light-palette` aliases). See the
  markdown-it README's "Dark mode" section for the three-line recipe.

Inspect `file.data.chromaglyph` for `{ cached, contentHash, css, warnings }`.

## Options

| Option | Default | Description |
|--------|---------|-------------|
| `font` | — | **Required.** Path or buffer of the source font: static TTF (TrueType) or OTF (name-keyed CFF), a WOFF1 container, or a variable TrueType font (pinned to its default instance; CFF2/CID-keyed CFF rejected). |
| `themes` | `['dark-plus', 'light-plus']` | Builtin theme names or paths to VS Code `.json` / TextMate `.tmTheme` files. |
| `langs` | `null` | Filter fences by language (`['rust', 'ts']`); `null` takes every fenced block. |
| `embed` | `true` | Inline the font as a data: URI; `false` emits files into `outDir`. |
| `outDir` | `'chromaglyph-out'` | Where emitted font/CSS assets go (`embed: false`). |
| `verify` | `true` | Run the rustybuzz oracle over the produced font before shipping it. |
| `semantic` | `false` | Overlay tree-sitter semantic colors (parameters, variables-as-scoped, types, calls) on top of the syntactic highlighting — rust/js/ts/python/java only; other languages silently stay syntactic. |
| `ligatures` | `false` | Preserve the source font's ligatures (`!=`, `->`, `www`, …): every substitution the font's own shaping performs is re-materialized as a build-time rule, so ligatures render exactly as the font intends while staying highlighted. Without it the source font's layout tables are dropped (they would fight the coloring rules) and ligature sequences render per-character. Ligature sites spanning multiple colors degrade to plain per-char text (colors win; warnings name them). |
| `format` | `'woff'` | Output container: `'woff2'` (Brotli, 15–30% smaller) or `'woff'`. CFF-flavored output (OTF sources) falls back to `'woff'` with a report warning; the generated CSS/data-URI mime always names the container that actually shipped. |
| `cacheDir` | — | Optional on-disk cache directory for incremental site builds. Ignored when `NODE_ENV=development` (dev-server HMR would write one font per intermediate edit state). |
| `grouping` | `'page'` | `'page'` batches all code blocks of a document into ONE font; `'lang'` builds one font per language — blocks carry `data-chromaglyph-lang` and the CSS scopes each font to its own language. Editing one language's snippet then rebuilds only that language's font, and identical text under different languages can highlight each its own way. Costs one subset base font (~10K) per extra language on a page. |

## License

Apache-2.0
