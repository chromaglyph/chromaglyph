# @chromaglyph/astro

Astro / Starlight integration for
[chromaglyph](https://github.com/chromaglyph/chromaglyph): syntax
highlighting compiled into a color font. The DOM text stays
byte-identical — no `<span>` soup, clean copy & paste.

## Starlight

One plugin entry wires everything: it disables Starlight's Shiki
highlighting (its inline-colored `<span>`s are exactly what chromaglyph
eliminates, and they fight the font's palette) and installs the remark
pipeline with the stylesheet as a `<link>`:

```js
// astro.config.mjs
import starlight from '@astrojs/starlight'
import { starlightPlugin } from '@chromaglyph/astro'
import { defineConfig } from 'astro/config'

export default defineConfig({
  integrations: [
    starlight({
      plugins: [
        starlightPlugin({
          font: './JetBrainsMono-Regular.ttf',
          cacheDir: '.chromaglyph-cache',
        }),
      ],
    }),
  ],
})
```

Remark plugins you configured yourself are preserved (chromaglyph is
appended after them).

## Plain Astro

Use [`@chromaglyph/remark`](../remark) directly — Astro's markdown
pipeline keeps raw html, so the default inline `<style>` works:

```js
// astro.config.mjs
import remarkChromaglyph from '@chromaglyph/remark'
import { defineConfig } from 'astro/config'

export default defineConfig({
  markdown: {
    syntaxHighlight: false, // turn Shiki off, the font does the coloring
    remarkPlugins: [[remarkChromaglyph, { font: './JBM.ttf' }]],
  },
})
```

## Options

`starlightPlugin(options)` accepts every
[`@chromaglyph/remark` option](../remark#options) except `style` (pinned
to `'link'`). `@astrojs/starlight` is an optional peer dependency — it
is only present in Starlight projects, which is the only place
`starlightPlugin` is used.

## License

Apache-2.0
