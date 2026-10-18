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
        // keep the font's own ligatures (!= -> === …) while highlighted;
        // mixed-color sites degrade to per-char text (colors win)
        ligatures: true,
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

The plugin's generated CSS defaults to `prefers-color-scheme` (OS level)
for the light/dark flip and bakes the theme's editor background into the
rules. Both are just **defaults behind CSS variables** — a class-toggle
framework overrides them in one place, with no runtime JS:

```css
/* .vitepress/theme/chromaglyph.css, imported from theme/index.ts */
:root {
  --chromaglyph-palette: var(--chromaglyph-light-palette);
  --chromaglyph-bg: var(--vp-code-block-bg); /* VitePress's own block bg */
}
html.dark {
  --chromaglyph-palette: var(--chromaglyph-dark-palette);
}
```

Why this works:

- The injected stylesheet defines **hash-free aliases**
  `--chromaglyph-dark-palette` / `--chromaglyph-light-palette` pointing at
  that page's palettes (they live in the same per-page stylesheet as the
  font, so SPA page swaps stay correct) and reads the applied palette as
  `font-palette: var(--chromaglyph-palette, …)`, the background as
  `background: var(--chromaglyph-bg, …)`.
- Once your CSS defines `--chromaglyph-palette`, it wins over **both** the
  dark default and the `prefers-color-scheme` branch — the media query
  becomes irrelevant and VitePress's `html.dark` class drives the flip
  (still a zero-reflow palette flip).
- `--chromaglyph-bg` detaches from the theme-baked `#1e1e1e`/`#ffffff`:
  pointing it at `var(--vp-code-block-bg)` makes the code block follow
  VitePress's own background system (radius, dark variant and all), on
  the `<pre>` **and** its `<code>` child together.

The same two vars work for any class-based toggle (Nuxt's `dark-mode`,
a `.dark` class you toggle yourself, …); frameworks that already follow
the OS scheme need nothing at all.

## Guarantees

- **Silent degradation**: any font-build failure renders the document
  unhighlighted (no stylesheet, no font) — your site build never breaks.
- **Cached**: the content hash is sha256(font + theme sources + block
  text + options + version), an in-process LRU plus an optional on-disk
  `cacheDir` (incremental site builds 49ms → 5ms).
- **Scoped**: `font-family` applies only to `[data-chromaglyph]`; prose is
  never affected.
