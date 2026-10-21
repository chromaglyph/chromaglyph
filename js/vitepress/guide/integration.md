# Integrations

chromaglyph ships adapters for the frameworks people actually ship docs
with. Each one exists because the framework's pipeline has a hard
constraint the generic plugin cannot guess — the adapter absorbs that
knowledge so you don't have to:

| Framework | Package | Pipeline | Stylesheet |
|-----------|---------|----------|------------|
| VitePress | `@chromaglyph/markdown-it` | markdown-it | `style: 'link'` |
| Astro / Starlight | `@chromaglyph/astro` | remark | `<link>` (forced) |
| Docusaurus | `@chromaglyph/docusaurus` | remark + MDX | `<link>` (forced) |
| Eleventy | `@chromaglyph/eleventy` | markdown-it | inline `<style>` |

The engine options (`font`, `themes`, `langs`, `grouping`, `semantic`,
`ligatures`, `format`, `embed`, `cacheDir`) mean the same thing
everywhere — see each package's README for the full table.

## VitePress

VitePress renders through markdown-it, so `@chromaglyph/markdown-it`
plugs in with two framework switches and one `md.use`:

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

### Why `style: 'link'`

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

### The appearance toggle

The generated CSS defaults to `prefers-color-scheme` for the light/dark flip
and to the theme's editor background. Both sit behind CSS variables, so a
class-based toggle overrides them in one place, no runtime JS:

```css
/* .vitepress/theme/style.css — the override goes ON the chromaglyph
   elements, never :root: var() substitution happens at the declaring
   element, and with grouping: 'lang' the palette aliases live on each
   [data-chromaglyph-lang] scope, invisible from :root. A :root-level
   override silently degrades to the dark fallback in BOTH modes. */
[data-chromaglyph],
[data-chromaglyph-lang] {
  --chromaglyph-palette: var(--chromaglyph-light-palette);
  --chromaglyph-bg: var(--vp-code-block-bg); /* VitePress's own block bg */
}
html.dark [data-chromaglyph],
html.dark [data-chromaglyph-lang] {
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

## Astro / Starlight

Starlight needs one plugin entry — it disables Shiki and installs the
remark pipeline for you:

```js
// astro.config.mjs
import starlight from '@astrojs/starlight'
import { starlightPlugin } from '@chromaglyph/astro'
import { defineConfig } from 'astro/config'

export default defineConfig({
  integrations: [
    starlight({
      plugins: [starlightPlugin({
        font: './JetBrainsMono-Regular.ttf',
        cacheDir: '.chromaglyph-cache',
      })],
    }),
  ],
})
```

Plain Astro (without Starlight) uses `@chromaglyph/remark` directly with
`markdown.syntaxHighlight: false` — Astro's pipeline keeps raw html, so
the default inline `<style>` works there.

## Docusaurus

Docusaurus renders markdown through **MDX**, which compiles the output to
JSX — the most constrained pipeline of the set. Both obvious stylesheet
shapes break: an injected `html` node dies in MDX's hast→estree step
("Cannot handle unknown node `raw`"), and CSS text in a `<style>` collides
with JSX expression syntax (`{}`). The adapter splits the work across
both plugin lists and ships the stylesheet as a data-URI `<link>`:

```js
// docusaurus.config.mjs
import {
  remarkChromaglyphDocusaurus,
  rehypeChromaglyphDocusaurus,
} from '@chromaglyph/docusaurus'

export default {
  markdown: {
    remarkPlugins: [[remarkChromaglyphDocusaurus, { font: './JBM.ttf' }]],
    rehypePlugins: [rehypeChromaglyphDocusaurus],
  },
  themeConfig: {
    prism: { disable: true }, // Prism's colored spans fight the palette
  },
}
```

## Eleventy

Eleventy renders through markdown-it; `@chromaglyph/eleventy` provides
the `amendLibrary` callback. Use an ESM config (the chromaglyph packages
are ESM-only, like the unified ecosystem they build on):

```js
// eleventy.config.mjs
import { withChromaglyph } from '@chromaglyph/eleventy'

export default function (eleventyConfig) {
  eleventyConfig.amendLibrary('md', withChromaglyph({
    font: './JetBrainsMono-Regular.ttf',
    cacheDir: '.chromaglyph-cache',
  }))
}
```

Eleventy's output is plain HTML, so the default inline `<style>` is the
right shape there.

## Guarantees

Every adapter inherits the engine's guarantees:

- **Silent degradation**: a failed font build renders the document
  unhighlighted (no stylesheet, no font); the site build never breaks.
- **Cached**: content hash = sha256(font + theme sources + block text +
  options + version), in-process LRU plus the optional on-disk `cacheDir`
  (incremental builds 49 ms → 5 ms).
- **Scoped**: `font-family` applies only to `[data-chromaglyph]`; prose is
  untouched.
