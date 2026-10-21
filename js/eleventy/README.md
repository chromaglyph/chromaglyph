# @chromaglyph/eleventy

Eleventy integration for [chromaglyph](https://github.com/chromaglyph/chromaglyph):
syntax highlighting compiled into a color font. The DOM text stays
byte-identical — no `<span>` soup, clean copy & paste.

Eleventy renders markdown through markdown-it, so highlighting drops in
via `amendLibrary`. Use an ESM config — the chromaglyph packages are
ESM-only, like the rest of the unified ecosystem:

```js
// eleventy.config.mjs (Eleventy 3)
import { withChromaglyph } from '@chromaglyph/eleventy'

export default function (eleventyConfig) {
  eleventyConfig.amendLibrary('md', withChromaglyph({
    font: './JetBrainsMono-Regular.ttf',
    cacheDir: '.chromaglyph-cache',
  }))
}
```

Eleventy output is plain HTML — nothing downstream compiles it as a
component template — so the stylesheet ships as an inline `<style>` by
default. All [`@chromaglyph/markdown-it` options](../markdown-it#options)
pass through unchanged; the dark-mode CSS-variable recipe and the
`grouping`/`semantic`/`ligatures`/`format` switches work exactly the
same.

## Options

See the `@chromaglyph/markdown-it` README — `withChromaglyph(options)`
forwards everything to the markdown-it plugin.

## License

Apache-2.0
