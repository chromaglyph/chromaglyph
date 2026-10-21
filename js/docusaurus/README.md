# @chromaglyph/docusaurus

Docusaurus integration for [chromaglyph](https://github.com/chromaglyph/chromaglyph):
syntax highlighting compiled into a color font. The DOM text stays
byte-identical — no `<span>` soup, clean copy & paste.

Docusaurus renders markdown through MDX, which compiles the output to
JSX. That rules out both shapes a plain remark pipeline would use for
the stylesheet:

- an injected `html` node dies in MDX's hast→estree step ("Cannot handle
  unknown node `raw`") — MDX only accepts *user-written* raw HTML,
  re-parsed by its own parser;
- raw CSS text inside a `<style>` node collides with JSX expression
  syntax (`{}`).

So this package ships the stylesheet as a data-URI
`<link rel="stylesheet">` — an ordinary element MDX renders happily —
split across the remark and rehype plugin lists:

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
}
```

Turn Docusaurus's own Prism highlighting off so it does not wrap code in
colored token spans that fight the font's palette:

```js
themeConfig: {
  prism: { disable: true },
},
```

The plugin is tested against the real `@mdx-js/mdx` compile pipeline —
including object literals (`{ a: 1 }`) inside fenced code, which is
where the JSX collision hazards live.

## Options

`remarkChromaglyphDocusaurus(options)` accepts every
[`@chromaglyph/remark` option](../remark#options) except `style` (pinned
to `'link'` — the only MDX-safe shape).

## License

Apache-2.0
