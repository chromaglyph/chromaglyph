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

### VitePress

VitePress renders through markdown-it, so the plugin drops in via the markdown
config — with two framework-level switches:

```ts
// .vitepress/config.mts
import { resolve } from 'node:path'
import { defineConfig } from 'vitepress'
import chromaglyph from '@chromaglyph/markdown-it'

export default defineConfig({
  markdown: {
    config(md) {
      // VitePress's built-in Shiki wraps tokens in colored <span>s — exactly
      // what chromaglyph eliminates. Hand the fence back to plain escaping.
      ;(md.options as { highlight: unknown }).highlight = null
      md.use(chromaglyph, {
        font: resolve(process.cwd(), 'JetBrainsMono-Regular.ttf'),
        style: 'link', // not 'inline': see below
        ligatures: true, // keep the font's != -> ligatures, highlighted
        cacheDir: '.chromaglyph-cache',
      })
    },
  },
})
```

- **`style: 'link'`**: Vue frameworks compile the markdown output as a Vue
  template. A `<style>` tag there is a side-effect tag — the client compiler
  errors out (dev serves a 404 for the page) and SSR entity-escapes the CSS
  text (breaking `@font-face`). A `<link rel="stylesheet">` is an ordinary
  element and survives both paths. Plain markdown-it pipelines keep the
  default `'inline'`.
- **`highlight = null`**: without it, Shiki's inline-colored spans paint every
  glyph and double-highlight against the font's palette.
- Fences are tagged `v-pre` so `{{ }}` inside code is never treated as Vue
  interpolation.

A runnable site lives in `js/vitepress` in the repo.

### Dark mode: `prefers-color-scheme` by default, one var to take over

With two themes the generated CSS flips dark/light automatically with the
**OS** color scheme and bakes each theme's editor background in. Sites
that toggle a **class** (VitePress's `html.dark`, Nuxt's color mode, …)
override both through two CSS variables — no runtime JS, and the override
wins in both modes at once:

```css
:root {
  --chromaglyph-palette: var(--chromaglyph-light-palette);
  --chromaglyph-bg: var(--vp-code-block-bg); /* or any bg you want */
}
html.dark {
  --chromaglyph-palette: var(--chromaglyph-dark-palette);
}
```

The injected stylesheet defines the hash-free aliases
`--chromaglyph-dark-palette` / `--chromaglyph-light-palette` (pointing at
that page's palettes — they live in the same per-page stylesheet as the
font, so SPA page swaps stay correct) and reads palette and background as
`var(--chromaglyph-palette, …)` / `var(--chromaglyph-bg, …)`. Pointing
`--chromaglyph-bg` at your framework's own code-block background variable
makes the block follow the framework's theming instead of the baked
`#1e1e1e`/`#ffffff`.

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
  `cacheDir`; the disk cache is skipped when `NODE_ENV=development` so a
  dev server's HMR edits cannot grow it without bound).
- `font-family` is scoped to `[data-chromaglyph]` so prose is never
  affected; themes become `@font-palette-values` (dark/light
  auto-switching when two themes are given).
- Introspection: `md.chromaglyph()` returns `{ cached, contentHash, css,
  warnings }` after a render.

## Options

| Option | Default | Description |
|--------|---------|-------------|
| `font` | — | **Required.** Path or buffer of the source font: static TTF (TrueType) or OTF (name-keyed CFF), a WOFF1 container, or a variable TrueType font (pinned to its default instance; CFF2/CID-keyed CFF rejected). |
| `themes` | `['dark-plus', 'light-plus']` | Builtin theme names or paths to VS Code `.json` / TextMate `.tmTheme` files. |
| `langs` | `null` | Filter fences by language (`['rust', 'ts']`); `null` takes every fenced block. |
| `embed` | `true` | Inline the font as a data: URI; `false` emits files into `outDir`. |
| `outDir` | `'chromaglyph-out'` | Where emitted font/CSS assets go (`embed: false`). |
| `style` | `'inline'` | `'inline'` injects a `<style>` tag; `'link'` injects a `<link rel="stylesheet">` (data: URI stylesheet, or a `.css` file with `embed: false`) — required for Vue-template frameworks like VitePress. |
| `verify` | `true` | Run the rustybuzz oracle over the produced font before shipping it. |
| `semantic` | `false` | Overlay tree-sitter semantic colors (parameters, variables-as-scoped, types, calls) on top of the syntactic highlighting — rust/js/ts/python/java only; other languages silently stay syntactic. |
| `ligatures` | `false` | Preserve the source font's ligatures (`!=`, `->`, `www`, …): every substitution the font's own shaping performs is re-materialized as a build-time rule, so ligatures render exactly as the font intends while staying highlighted. Without it the source font's layout tables are dropped (they would fight the coloring rules) and ligature sequences render per-character. Ligature sites spanning multiple colors degrade to plain per-char text (colors win; warnings name them). |
| `format` | `'woff'` | Output container: `'woff2'` (Brotli, 15–30% smaller) or `'woff'`. CFF-flavored output (OTF sources) falls back to `'woff'` with a report warning; the generated CSS/data-URI mime always names the container that actually shipped. |
| `cacheDir` | — | Optional on-disk cache directory for incremental site builds. Ignored when `NODE_ENV=development` (dev-server HMR would write one font per intermediate edit state). |
| `grouping` | `'page'` | `'page'` batches all fences of a render into ONE font; `'lang'` builds one font per language — fences carry `data-chromaglyph-lang` and the CSS scopes each font to its own language's blocks. Editing one language's snippet then rebuilds only that language's font (the others hit the cache), and identical text under different languages can highlight each its own way. Costs one subset base font (~10K) per extra language on a page. |

## License

Apache-2.0
