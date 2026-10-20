# VitePress integration

VitePress renders through markdown-it, so `@chromaglyph/markdown-it` plugs
in with two framework switches and one `md.use`:

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

## Why `style: 'link'`

Vue frameworks compile markdown output **as a Vue template**, and the
plugin's default inline `<style>` does not survive it:

- **dev**: the client compiler rejects `<style>` as a side-effect tag — the
  page 404s;
- **build (SSG)**: SSR entity-escapes the CSS (`'` → `&#39;`), browsers do
  not decode entities inside `<style>`, and `@font-face` dies.

A `<link rel="stylesheet" href="data:text/css;base64,...">` is a plain
element whose attributes survive both. The `v-pre` the plugin puts on every
`<pre>` matters for the same reason — without it Vue reads `{{ }}` in code
as interpolation.

## The appearance toggle

The generated CSS defaults to `prefers-color-scheme` for the light/dark flip
and to the theme's editor background. Both sit behind CSS variables, so a
class-based toggle overrides them in one place, no runtime JS:

```css
/* .vitepress/theme/style.css */
:root {
  --chromaglyph-palette: var(--chromaglyph-light-palette);
  --chromaglyph-bg: var(--vp-code-block-bg); /* VitePress's own block bg */
}
html.dark {
  --chromaglyph-palette: var(--chromaglyph-dark-palette);
}
```

- The per-page stylesheet defines hash-free aliases
  `--chromaglyph-dark-palette` / `--chromaglyph-light-palette` and reads
  `font-palette: var(--chromaglyph-palette, …)`. Once you set
  `--chromaglyph-palette`, it beats both the dark default and the media
  query; VitePress's `html.dark` drives the flip — still zero reflow.
- `--chromaglyph-bg` detaches the block from the theme-baked
  `#1e1e1e`/`#ffffff`; pointing it at `var(--vp-code-block-bg)` makes the
  `<pre>` and its `<code>` follow VitePress's own background system.

The same two variables serve any class-based toggle (Nuxt's `dark-mode`, a
`.dark` you flip yourself). Sites that follow the OS scheme need nothing.

## Guarantees

- **Silent degradation**: a failed font build renders the document
  unhighlighted (no stylesheet, no font); the site build never breaks.
- **Cached**: content hash = sha256(font + theme sources + block text +
  options + version), in-process LRU plus the optional on-disk `cacheDir`
  (incremental builds 49 ms → 5 ms).
- **Scoped**: `font-family` applies only to `[data-chromaglyph]`; prose is
  untouched.
