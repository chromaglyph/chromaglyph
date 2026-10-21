import { test } from 'vitest'
import assert from 'node:assert/strict'
import { existsSync, readFileSync, readdirSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { unified } from 'unified'
import remarkParse from 'remark-parse'
import remarkRehype from 'remark-rehype'
import rehypeStringify from 'rehype-stringify'
import remarkChromaglyph, { type ChromaglyphFileInfo, type RemarkChromaglyphOptions } from '../src/index'
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

async function render(markdown: string, options: RemarkChromaglyphOptions) {
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
  assert.ok(data!.css.includes('@font-face'), 'css contains font-face')
  assert.ok(data!.css.includes('data:font/woff;base64,'), 'font embedded as data URI')
  assert.ok(data!.css.includes('base-palette: 1'), 'light palette present')
  assert.ok(html.includes('<style>'), 'style injected into html')
  assert.equal((html.match(/data-chromaglyph=/g) || []).length, 2, 'both blocks marked')
})

test('code text stays byte-identical in the DOM', async () => {
  const { html } = await render(MARKDOWN, { font: FONT })
  assert.ok(html.includes('fn main() {\n    let msg = "hello chromaglyph";\n}'))
  assert.ok(html.includes('const n = 42; // the answer'))
  // inline code in prose is NOT marked (never audited => never font-scoped)
  assert.ok(!/<code[^>]*data-chromaglyph[^>]*>fn main\(\)<\/code>/.test(html), 'inline code never carries the attribute')
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
  const a = first.data as Extract<ChromaglyphFileInfo, { grouping?: undefined }>
  const b = second.data as Extract<ChromaglyphFileInfo, { grouping?: undefined }>
  assert.equal(a.cached, false)
  assert.equal(b.cached, true)
  assert.equal(a.contentHash, b.contentHash)
})

test("grouping: 'lang' builds one scoped font per language", async () => {
  clearCache()
  const { html, data } = await render(MARKDOWN, { font: FONT, grouping: 'lang' })
  assert.ok(data!.grouping === 'lang')
  const info = data! as Extract<ChromaglyphFileInfo, { grouping: 'lang' }>
  assert.equal(info.fonts.length, 2, 'one font per language')
  const langs = info.fonts.map((f) => f.lang).sort()
  assert.deepEqual(langs, ['javascript', 'rust'])
  assert.ok(html.includes('data-chromaglyph-lang="rust"'), 'rust block tagged')
  assert.ok(html.includes('data-chromaglyph-lang="javascript"'), 'js block tagged')
  assert.equal((info.css.match(/@font-face/g) || []).length, 2)
  assert.ok(info.css.includes('[data-chromaglyph-lang="rust"]'))
  assert.ok(!info.css.includes(':root'), 'aliases are lang-scoped, not :root')
  // two distinct families, one style tag injected
  const families = new Set(info.css.match(/font-family: chromaglyph-[0-9a-f]{8}/g))
  assert.equal(families.size, 2)
  assert.equal((html.match(/<style>/g) || []).length, 1)
})

test('grouping rejects unknown values', async () => {
  await assert.rejects(() => render(MARKDOWN, { font: FONT, grouping: 'nope' as 'page' }), /unknown grouping/)
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
  const info = data as Extract<ChromaglyphFileInfo, { grouping?: undefined }>
  assert.ok(info.fontUrl.startsWith('./chromaglyph-'), `fontUrl: ${info.fontUrl}`)
  assert.match(info.css, /src: url\(\.\/chromaglyph-[0-9a-f]+\.woff\)/)
})

test('semantic option produces its own build (distinct hash, same pipeline)', async () => {
  const md = '```rust\nfn area(r: f64) -> f64 { let p = r * 2.0; p }\n```\n'
  const plain = await render(md, { font: FONT, verify: false })
  const semantic = await render(md, { font: FONT, verify: false, semantic: true })
  assert.ok(plain.data!.css.includes('@font-face'))
  assert.ok(semantic.data!.css.includes('@font-face'))
  const a = plain.data as Extract<ChromaglyphFileInfo, { grouping?: undefined }>
  const b = semantic.data as Extract<ChromaglyphFileInfo, { grouping?: undefined }>
  assert.notEqual(a.contentHash, b.contentHash, 'semantic joins the content hash')
})

test("format: 'woff2' embeds a woff2 data URI", async () => {
  clearCache()
  const { data } = await render(MARKDOWN, { font: FONT, format: 'woff2' })
  assert.ok(data!.css.includes('data:font/woff2;base64,'), 'woff2 data URI')
  assert.ok(data!.css.includes("format('woff2')"), 'css names the woff2 container')
  assert.ok(!data!.css.includes('data:font/woff;'), 'not the woff1 mime')
})

test("style: 'link' injects a stylesheet link, not a <style> node (MDX/Vue pipelines)", async () => {
  clearCache()
  const { html, data } = await render(MARKDOWN, { font: FONT, style: 'link' })
  assert.ok(!html.includes('<style>'), 'no style node — MDX would parse it as JSX with { } hazards')
  assert.equal((html.match(/<link rel="stylesheet" href="data:text\/css;base64,/g) || []).length, 1)
  const b64 = html.match(/href="data:text\/css;base64,([^"]+)"/)![1]!
  const css = Buffer.from(b64, 'base64').toString('utf8')
  assert.ok(css.includes('@font-face'), 'decoded stylesheet carries the font')
  assert.ok(data!.css.includes('@font-face'), 'file.data.chromaglyph still exposes the raw css')
})

test("style: 'link' with embed: false writes a css file next to the woff", async () => {
  clearCache()
  const outDir = '/tmp/cg-remark-link-file'
  rmSync(outDir, { recursive: true, force: true })
  const { html } = await render(MARKDOWN, { font: FONT, style: 'link', embed: false, outDir })
  assert.match(html, /<link rel="stylesheet" href="\.\/chromaglyph-[0-9a-f]+\.css">/)
  const cssFile = readdirSync(outDir).find((f) => f.endsWith('.css'))
  assert.ok(cssFile, 'css file emitted')
  assert.ok(readFileSync(join(outDir, cssFile!), 'utf8').includes('@font-face'))
})

test("style: 'link' + grouping: 'lang' injects one link per language", async () => {
  clearCache()
  const { html } = await render(MARKDOWN, { font: FONT, style: 'link', grouping: 'lang' })
  assert.equal((html.match(/<link rel="stylesheet" href="data:text\/css;base64,/g) || []).length, 2)
  assert.ok(!html.includes('<style>'))
})
