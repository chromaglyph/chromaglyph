import { test } from 'vitest'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import MarkdownIt from 'markdown-it'
import { withChromaglyph } from '../src/index'

const FONT = readFileSync('../../crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf')
const MARKDOWN = `# demo

\`\`\`rust
fn main() {
    let msg = "hello chromaglyph";
}
\`\`\`
`

test('the amendLibrary callback wires chromaglyph into an Eleventy markdown-it', () => {
  const md = new MarkdownIt({ html: true })
  // exactly what `eleventyConfig.amendLibrary('md', withChromaglyph(...))` runs
  withChromaglyph({ font: FONT })(md)
  const html = md.render(MARKDOWN)
  assert.ok(html.includes('<style>'), 'static-HTML pipeline gets an inline stylesheet')
  assert.ok(html.includes('@font-face'), 'stylesheet carries the font')
  assert.ok(/<pre v-pre data-chromaglyph="0"/.test(html), 'code block marked')
})
