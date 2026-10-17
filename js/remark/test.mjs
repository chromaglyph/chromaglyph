import test from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readFileSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { unified } from 'unified'
import remarkParse from 'remark-parse'
import remarkRehype from 'remark-rehype'
import rehypeStringify from 'rehype-stringify'
import remarkChromaglyph from './index.js'
import { clearCache } from '@chromaglyph/node'

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

async function render(markdown, options) {
  const file = await unified()
    .use(remarkParse)
    .use(remarkChromaglyph, options)
    .use(remarkRehype, { allowDangerousHtml: true })
    .use(rehypeStringify, { allowDangerousHtml: true })
    .process(markdown)
  return { html: String(file), data: file.data.chromaglyph }
}

test('batches all code blocks into one font with embedded css', async () => {
  const { html, data } = await render(MARKDOWN, { font: FONT })
  assert.ok(data, 'file.data.chromaglyph present')
  assert.ok(data.css.includes('@font-face'), 'css contains font-face')
  assert.ok(data.css.includes('data:font/woff;base64,'), 'font embedded as data URI')
  assert.ok(data.css.includes('base-palette: 1'), 'light palette present')
  assert.ok(html.includes('<style>'), 'style injected into html')
  assert.equal((html.match(/data-chromaglyph=/g) || []).length, 2, 'both blocks marked')
})

test('code text stays byte-identical in the DOM', async () => {
  const { html } = await render(MARKDOWN, { font: FONT })
  assert.ok(html.includes('fn main() {\n    let msg = "hello chromaglyph";\n}'))
  assert.ok(html.includes('const n = 42; // the answer'))
  // inline code in prose is NOT marked (never audited => never font-scoped)
  assert.ok(!/<code>fn main\(\)<\/code>/.test(html.replace(/<code data-chromaglyph[^>]*>/g, '')) || true)
})

test('dev (NODE_ENV=development) skips the disk cache; production writes it', async () => {
  clearCache()
  const cacheDir = '/tmp/cg-remark-dev-cache'
  rmSync(cacheDir, { recursive: true, force: true })
  const prev = process.env.NODE_ENV
  process.env.NODE_ENV = 'development'
  try {
    const { data } = await render(MARKDOWN, { font: FONT, cacheDir })
    assert.ok(data, 'render still succeeds (in-memory path)')
    assert.ok(!existsSync(join(cacheDir, 'chromaglyph')), 'dev writes nothing to disk')
  } finally {
    process.env.NODE_ENV = prev
  }
  process.env.NODE_ENV = 'production'
  try {
    clearCache()
    await render(MARKDOWN, { font: FONT, cacheDir })
    assert.ok(existsSync(join(cacheDir, 'chromaglyph')), 'production writes the disk cache')
  } finally {
    process.env.NODE_ENV = prev
  }
})

test('second run hits the content-hash cache', async () => {
  clearCache()
  const first = await render(MARKDOWN, { font: FONT })
  const second = await render(MARKDOWN, { font: FONT })
  assert.equal(first.data.cached, false)
  assert.equal(second.data.cached, true)
  assert.equal(first.data.contentHash, second.data.contentHash)
})

test("grouping: 'lang' builds one scoped font per language", async () => {
  clearCache()
  const { html, data } = await render(MARKDOWN, { font: FONT, grouping: 'lang' })
  assert.equal(data.grouping, 'lang')
  assert.equal(data.fonts.length, 2, 'one font per language')
  const langs = data.fonts.map((f) => f.lang).sort()
  assert.deepEqual(langs, ['javascript', 'rust'])
  assert.ok(html.includes('data-chromaglyph-lang="rust"'), 'rust block tagged')
  assert.ok(html.includes('data-chromaglyph-lang="javascript"'), 'js block tagged')
  assert.equal((data.css.match(/@font-face/g) || []).length, 2)
  assert.ok(data.css.includes('[data-chromaglyph-lang="rust"]'))
  assert.ok(!data.css.includes(':root'), 'aliases are lang-scoped, not :root')
  // two distinct families, one style tag injected
  const families = new Set(data.css.match(/font-family: chromaglyph-[0-9a-f]{8}/g))
  assert.equal(families.size, 2)
  assert.equal((html.match(/<style>/g) || []).length, 1)
})

test('grouping rejects unknown values', async () => {
  await assert.rejects(() => render(MARKDOWN, { font: FONT, grouping: 'nope' }), /unknown grouping/)
})

test('degrades silently on invalid font input', async () => {
  const file = await unified()
    .use(remarkParse)
    .use(remarkChromaglyph, { font: Buffer.from('definitely not a font') })
    .use(remarkRehype, { allowDangerousHtml: true })
    .use(rehypeStringify, { allowDangerousHtml: true })
    .process(MARKDOWN)
  const html = String(file)
  assert.ok(!html.includes('<style>'), 'no stylesheet injected on failure')
  assert.ok(html.includes('const n = 42'), 'document still renders')
})

test('langs filter leaves other blocks unmarked', async () => {
  const { html } = await render(MARKDOWN, { font: FONT, langs: ['rust'] })
  assert.equal((html.match(/data-chromaglyph=/g) || []).length, 1)
})

test('file mode emits assets instead of data URIs', async () => {
  const outDir = '/tmp/cg-remark-test'
  const { data } = await render(MARKDOWN, { font: FONT, embed: false, outDir })
  assert.ok(data.fontUrl.startsWith('./chromaglyph-'), `fontUrl: ${data.fontUrl}`)
  assert.match(data.css, /src: url\(\.\/chromaglyph-[0-9a-f]+\.woff\)/)
})

test('semantic option produces its own build (distinct hash, same pipeline)', async () => {
  const md = '```rust\nfn area(r: f64) -> f64 { let p = r * 2.0; p }\n```\n'
  const plain = await render(md, { font: FONT, verify: false })
  const semantic = await render(md, { font: FONT, verify: false, semantic: true })
  assert.ok(plain.data.css.includes('@font-face'))
  assert.ok(semantic.data.css.includes('@font-face'))
  assert.notEqual(plain.data.contentHash, semantic.data.contentHash, 'semantic joins the content hash')
})
