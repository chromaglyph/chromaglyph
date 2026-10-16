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

- **批处理合并**：all fenced code blocks of a document batch into ONE font
  (multi-language global disambiguation included)
- **零配置**：the font is inlined as a data: URI in an injected `<style>`
  (set `embed: false` + `outDir` to emit files instead)
- **静默降级**：any build failure renders the document unhighlighted — your
  site build never breaks
- **缓存**：content-hash cache (in-process + optional on-disk `cacheDir`)
- Scopes `font-family` to `[data-chromaglyph]` so prose is never affected;
  themes become `@font-palette-values` (dark/light auto-switching when two
  themes are given)

Inspect `file.data.chromaglyph` for `{ cached, contentHash, css, warnings }`.
