import { test } from 'vitest'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { compile } from '@mdx-js/mdx'
import remarkChromaglyphDocusaurus, {
  rehypeChromaglyphDocusaurus,
} from '../src/index'

const FONT = readFileSync('../../crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf')

// MDX source shaped like a real Docusaurus docs page: fenced code plus
// braces everywhere — in code and in prose — to prove the JSX collision
// hazards this package exists to prevent
const PAGE = `# demo

Some prose with \`fn main()\` inline code.

\`\`\`rust
fn main() {
    let msg = "hello chromaglyph";
    let m = { a: 1 };
}
\`\`\`
`

async function compileMdx() {
  // exactly what docusaurus.config's markdown: { remarkPlugins, rehypePlugins } runs
  const out = await compile(PAGE, {
    remarkPlugins: [[remarkChromaglyphDocusaurus, { font: FONT }]],
    rehypePlugins: [rehypeChromaglyphDocusaurus],
  })
  return String(out)
}

test('MDX pipeline: stylesheet renders as a <link> element, never a <style> node', async () => {
  const js = await compileMdx()
  assert.ok(!js.includes('<style'), 'a <style> node would collide with JSX expressions in MDX')
  // MDX compiles elements to `_jsx(_components.link, { rel, href })`, where
  // `_components` carries a `link: "link"` tag-name fallback — a real
  // <link> reaches the DOM, not a text string
  assert.ok(/_jsx\(_components\.link, \{\s*rel: "stylesheet"/.test(js), 'link element call present')
  const b64 = js.match(/href: "data:text\/css;base64,([^"]+)"/)?.[1]
  assert.ok(b64, 'stylesheet href is a data URI')
  const css = Buffer.from(b64!, 'base64').toString('utf8')
  assert.ok(css.includes('@font-face'), 'decoded stylesheet carries the font')
})

test('MDX pipeline: code blocks carry the chromaglyph attributes', async () => {
  const js = await compileMdx()
  assert.ok(/"data-chromaglyph": "0"/.test(js), 'code block marked (JSX property form)')
})

test('braces inside code and prose compile through the whole MDX pipeline', async () => {
  const js = await compileMdx()
  assert.ok(js.includes('let m = { a: 1 }'), 'object literal survives MDX compilation')
})
