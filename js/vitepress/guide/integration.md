# VitePress integration

`@chromaglyph/markdown-it` is the channel built for VitePress (VitePress
renders through markdown-it). Two framework-level switches plus one
`md.use`:

```ts
// .vitepress/config.mts
import { resolve } from 'node:path'
import { defineConfig } from 'vitepress'
import chromaglyph from '@chromaglyph/markdown-it'

export default defineConfig({
  markdown: {
    config(md) {
      // 1) Disable Shiki: it wraps tokens in colored <span>s, which is
      //    exactly what chromaglyph eliminates; hand the fence back to
      //    markdown-it's plain escaping rule
      ;(md.options as { highlight: unknown }).highlight = null
      // 2) style: 'link' - see below
      md.use(chromaglyph, {
        font: resolve(process.cwd(), 'JetBrainsMono-Regular.ttf'),
        themes: ['dark-plus', 'light-plus'],
        style: 'link',
        cacheDir: '.chromaglyph-cache',
      })
    },
  },
})
```

## Why `style: 'link'` is required

Vue-based frameworks compile the markdown output **as a Vue template**,
and the plugin's default inline `<style>` tag cannot survive that
pipeline:

- **dev**: the client compiler treats `<style>` as a side-effect tag and
  throws — the page serves a 404;
- **build (SSG)**: SSR entity-escapes the CSS as a text node (quotes
  become `&#39;`), and browsers do not decode entities inside `<style>`,
  killing `@font-face`.

A `<link rel="stylesheet" href="data:text/css;base64,...">` is an ordinary
element whose attributes survive both paths. The `v-pre` the plugin adds
to every `<pre>` is equally necessary — without it Vue would treat `{{ }}`
inside code as interpolation.

## The appearance toggle

The plugin's generated CSS follows `prefers-color-scheme` (OS level),
while VitePress's toggle button switches the `html.dark` class. Note that
the plugin's scope rule paints the background on both the `<pre>` and its
`<code>` child — a shim that only flips the `<pre>` leaves a dark `<code>`
behind. This site's `.vitepress/theme/chromaglyph-palette.ts` is a ~40-line
example that wires them together: at runtime it reads the injected
`@font-palette-values` rule names and generates a `html.dark` /
`html:not(.dark)` scoped stylesheet (prefixing every selector in the
comma list, so both `pre` and `code` flip) — a palette flip with zero
reflow.

## Guarantees

- **Silent degradation**: any font-build failure renders the document
  unhighlighted (no stylesheet, no font) — your site build never breaks.
- **Cached**: the content hash is sha256(font + theme sources + block
  text + options + version), an in-process LRU plus an optional on-disk
  `cacheDir` (incremental site builds 49ms → 5ms).
- **Scoped**: `font-family` applies only to `[data-chromaglyph]`; prose is
  never affected.
