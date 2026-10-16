import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import MarkdownIt from 'markdown-it'
import { clearCache } from '@chromaglyph/node'
import chromaglyph from './index.js'

const FONT = readFileSync('../../crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf')
const MARKDOWN = `# demo

Intro with \`fn main()\` inline code that must stay untouched.

\`\`\`rust
fn main() {
    let msg = "hello chromaglyph";
}
\`\`\`

\`\`\`javascript
const n = 42; // the answer
\`\`\`
`

function makeMd(options = {}) {
  const md = new MarkdownIt({ html: true })
  md.use(chromaglyph, { font: FONT, ...options })
  return md
}

test('batches all fences into one font with a single injected style', () => {
  clearCache()
  const md = makeMd()
  const html = md.render(MARKDOWN)
  assert.equal((html.match(/<style>/g) || []).length, 1, 'one stylesheet')
  assert.ok(html.includes('@font-face'), 'css contains font-face')
  assert.ok(html.includes('data:font/woff;base64,'), 'font embedded as data URI')
  assert.equal((html.match(/data-chromaglyph=/g) || []).length, 2, 'both fences marked')
})

test('code text stays byte-identical (escaped, not transformed)', () => {
  const plain = new MarkdownIt().render(MARKDOWN)
  const ours = makeMd().render(MARKDOWN)
  const strip = (s) => s.replace(/<style>[\s\S]*?<\/style>\n?/, '').replace(/ data-chromaglyph="\d+"/g, '')
  assert.equal(strip(ours), strip(plain))
})

test('inline code in prose is never marked', () => {
  const html = makeMd().render(MARKDOWN)
  assert.ok(html.includes('<code>fn main()</code>'), 'inline code renders plain')
})

test('second render of same source hits the cache', () => {
  clearCache()
  const md = makeMd()
  md.render(MARKDOWN)
  const first = md.chromaglyph()
  md.render(MARKDOWN)
  const second = md.chromaglyph()
  assert.equal(first.cached, false)
  assert.equal(second.cached, true)
})

test('degrades silently on invalid font', () => {
  const md = new MarkdownIt({ html: true })
  md.use(chromaglyph, { font: Buffer.from('not a font') })
  const html = md.render(MARKDOWN)
  assert.ok(!html.includes('<style>'))
  assert.ok(html.includes('const n = 42'), 'document still renders')
})

test('langs filter leaves other fences untouched', () => {
  const html = makeMd({ langs: ['rust'] }).render(MARKDOWN)
  assert.equal((html.match(/data-chromaglyph=/g) || []).length, 1)
})

test('chained after another fence rule (VitePress-like)', () => {
  const md = new MarkdownIt({ html: true })
  // simulate a site that already customizes fence rendering
  md.renderer.rules.fence = (tokens, idx) => {
    const token = tokens[idx]
    return `<pre class="vp-code"><code>${md.utils.escapeHtml(token.content)}</code></pre>\n`
  }
  md.use(chromaglyph, { font: FONT })
  const html = md.render(MARKDOWN)
  assert.ok(
    /<pre[^>]*class="vp-code"[^>]*>/.test(html) && /<pre[^>]*data-chromaglyph="0"[^>]*>/.test(html),
    `chains onto custom fence: ${html.slice(0, 200)}`
  )
  assert.ok(html.includes('<style>'))
})

test('file mode emits assets', () => {
  const outDir = '/tmp/cg-mdit-test'
  const md = makeMd({ embed: false, outDir })
  const html = md.render(MARKDOWN)
  assert.match(html, /src: url\(\.\/chromaglyph-[0-9a-f]+\.woff\)/)
})
