import test from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readFileSync, readdirSync, rmSync } from 'node:fs'
import { join } from 'node:path'
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
  const strip = (s) => s.replace(/<style>[\s\S]*?<\/style>\n?/, '').replace(/ v-pre data-chromaglyph="\d+"/g, '')
  assert.equal(strip(ours), strip(plain))
})

test('style: link injects a stylesheet link, not a <style> tag (Vue-template frameworks)', () => {
  clearCache()
  const html = makeMd({ style: 'link' }).render(MARKDOWN)
  assert.ok(!html.includes('<style>'), 'no style tag — Vue client compile would throw on it')
  assert.equal((html.match(/<link rel="stylesheet" href="data:text\/css;base64,/g) || []).length, 1)
  const b64 = html.match(/href="data:text\/css;base64,([^"]+)"/)[1]
  const css = Buffer.from(b64, 'base64').toString('utf8')
  assert.ok(css.includes('@font-face'), 'decoded stylesheet carries the font')
  assert.ok(css.includes('@font-palette-values'), 'decoded stylesheet carries palettes')
  // v-pre 必须在，否则代码里的 {{ }} 会被 Vue 当插值
  assert.ok(/<pre v-pre data-chromaglyph="\d+"/.test(html), 'pre carries v-pre')
})

test('generated css is var-driven with hash-free palette aliases', () => {
  clearCache()
  const html = makeMd({ style: 'link' }).render(MARKDOWN)
  const b64 = html.match(/href="data:text\/css;base64,([^"]+)"/)[1]
  const css = Buffer.from(b64, 'base64').toString('utf8')
  // hash-free aliases let a site override the palettes without knowing
  // the per-page hash — the class-toggle dark mode recipe uses them
  assert.ok(css.includes('--chromaglyph-dark-palette: --chromaglyph-'))
  assert.ok(css.includes('--chromaglyph-light-palette: --chromaglyph-'))
  assert.ok(
    css.includes('font-palette: var(--chromaglyph-palette, var(--chromaglyph-dark-palette))'),
    'dark default defers to --chromaglyph-palette',
  )
  assert.ok(
    css.includes('font-palette: var(--chromaglyph-palette, var(--chromaglyph-light-palette))'),
    'prefers-color-scheme branch defers to the same var',
  )
  assert.ok(css.includes('background: var(--chromaglyph-bg, '), 'background is var-driven')
})

test('style: link with embed: false writes a css file next to the woff', () => {
  clearCache()
  const outDir = '/tmp/cg-mdit-link-file'
  const html = makeMd({ style: 'link', embed: false, outDir }).render(MARKDOWN)
  assert.match(html, /<link rel="stylesheet" href="\.\/chromaglyph-[0-9a-f]+\.css">/)
  const cssFile = readdirSync(outDir).find((f) => f.endsWith('.css'))
  assert.ok(cssFile, 'css file emitted')
  assert.ok(readFileSync(join(outDir, cssFile), 'utf8').includes('@font-face'))
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

test('a wrapper chain around md.render (VitePress-like sfc hoisting) keeps working', () => {
  // VitePress assembles its markdown features as wrappers around the render
  // entry points: its sfc plugin initializes env.sfcBlocks inside its own
  // md.render wrapper and its html_block rule hoists top-level
  // <script setup>/<style> blocks out of the template from there. chromaglyph
  // used to replace md.render with a from-scratch parse+render, bypassing
  // that wrapper — env.sfcBlocks stayed unset, the script stayed in the
  // html, and Vue's client compiler rejected the page ("Tags with side
  // effect ... ignored in client component templates").
  const md = new MarkdownIt({ html: true })
  // host wrappers install BEFORE the user's markdown.config (chromaglyph)
  const prevHtmlBlock = md.renderer.rules.html_block
  md.renderer.rules.html_block = (tokens, idx, opts, env, self) => {
    const hoisted = env.sfcBlocks
    if (!hoisted || !/^<script[\s\S]*<\/script>$/.test(tokens[idx].content.trim())) {
      return prevHtmlBlock(tokens, idx, opts, env, self)
    }
    hoisted.scripts.push(tokens[idx].content)
    return ''
  }
  const render = md.render.bind(md)
  md.render = (src, env = {}) => {
    env.sfcBlocks = { scripts: [] }
    return render(src, env)
  }
  md.use(chromaglyph, { font: FONT })

  const html = md.render(
    '# t\n\n```rust\nfn main() {}\n```\n\n<script setup>\nconst x = 1\n</script>\n',
  )
  assert.ok(!html.includes('<script'), `script must be hoisted out of the html: ${html}`)
  assert.ok(html.includes('<style>'), 'chromaglyph still injects its stylesheet')
  assert.ok(/data-chromaglyph="0"/.test(html), 'fence still marked')
})

test('file mode emits assets', () => {
  const outDir = '/tmp/cg-mdit-test'
  const md = makeMd({ embed: false, outDir })
  const html = md.render(MARKDOWN)
  assert.match(html, /src: url\(\.\/chromaglyph-[0-9a-f]+\.woff\)/)
})

test('dev (NODE_ENV=development) skips the disk cache; production writes it', () => {
  clearCache()
  const cacheDir = '/tmp/cg-mdit-dev-cache'
  rmSync(cacheDir, { recursive: true, force: true })
  const prev = process.env.NODE_ENV
  process.env.NODE_ENV = 'development'
  try {
    makeMd({ cacheDir }).render(MARKDOWN)
    assert.ok(!existsSync(join(cacheDir, 'chromaglyph')), 'dev writes nothing to disk')
  } finally {
    process.env.NODE_ENV = prev
  }
  process.env.NODE_ENV = 'production'
  try {
    clearCache()
    makeMd({ cacheDir }).render(MARKDOWN)
    assert.ok(existsSync(join(cacheDir, 'chromaglyph')), 'production writes the disk cache')
  } finally {
    process.env.NODE_ENV = prev
  }
})

test("grouping: 'lang' ships one scoped font per language", () => {
  clearCache()
  const html = makeMd({ grouping: 'lang', style: 'link' }).render(MARKDOWN)
  // one stylesheet link per language, both injected before the first fence
  assert.equal((html.match(/<link rel="stylesheet" href="data:text\/css;base64,/g) || []).length, 2)
  const css = [...html.matchAll(/href="data:text\/css;base64,([^"]+)"/g)]
    .map((m) => Buffer.from(m[1], 'base64').toString('utf8'))
    .join('\n')
  assert.equal((css.match(/@font-face/g) || []).length, 2, 'one font-face per language')
  assert.ok(html.includes('<pre v-pre data-chromaglyph="0" data-chromaglyph-lang="rust"'), 'rust fence tagged')
  assert.ok(html.includes('data-chromaglyph-lang="javascript"'), 'js fence tagged')
  assert.ok(css.includes('[data-chromaglyph-lang="rust"]'), 'rust rules scoped')
  assert.ok(css.includes('[data-chromaglyph-lang="javascript"]'), 'js rules scoped')
  // palette aliases live on the lang scope, not :root, so the fonts never
  // fight over the alias names
  assert.ok(!css.includes(':root'), 'no :root aliases in lang mode')
  assert.ok(css.includes('[data-chromaglyph-lang="rust"] {\n  --chromaglyph-dark-palette:'))
  // two distinct families
  const families = new Set(css.match(/font-family: chromaglyph-[0-9a-f]{8}/g))
  assert.equal(families.size, 2)
  // introspection: one BuildResult per language
  const md2 = makeMd({ grouping: 'lang' })
  md2.render(MARKDOWN)
  assert.equal(md2.chromaglyph().length, 2)
})

test("grouping: 'lang' rebuilds only the edited language's font", () => {
  clearCache()
  const md = makeMd({ grouping: 'lang' })
  const page = (rustBody) => `\`\`\`rust\n${rustBody}\n\`\`\`\n\n\`\`\`javascript\nconst n = 42; // the answer\n\`\`\`\n`
  md.render(page('let a = 1;'))
  const first = md.chromaglyph().map((r) => r.contentHash)
  md.render(page('let a = 1;'))
  assert.ok(md.chromaglyph().every((r) => r.cached), 'unchanged page hits the cache')
  md.render(page('let a = 2;'))
  const after = md.chromaglyph()
  assert.equal(after[0].cached, false, 'edited rust group rebuilds')
  assert.equal(after[1].cached, true, 'untouched js group hits the cache')
  assert.notEqual(after[0].contentHash, first[0])
  assert.equal(after[1].contentHash, first[1])
})

test('grouping rejects unknown values', () => {
  assert.throws(() => makeMd({ grouping: 'nope' }), /unknown grouping/)
})

test('semantic option produces its own build (distinct hash)', () => {
  const md = new MarkdownIt({ html: true })
  md.use(chromaglyph, { font: FONT, verify: false })
  md.render('```rust\nfn area(r: f64) -> f64 { let p = r * 2.0; p }\n```\n')
  const plain = md.chromaglyph().contentHash
  const md2 = new MarkdownIt({ html: true })
  md2.use(chromaglyph, { font: FONT, verify: false, semantic: true })
  md2.render('```rust\nfn area(r: f64) -> f64 { let p = r * 2.0; p }\n```\n')
  const semantic = md2.chromaglyph().contentHash
  assert.notStrictEqual(plain, semantic, 'semantic joins the content hash')
})

test("format: 'woff2' embeds a woff2 data URI", () => {
  clearCache()
  const md = makeMd({ format: 'woff2' })
  const html = md.render(MARKDOWN)
  assert.ok(html.includes('data:font/woff2;base64,'), 'woff2 data URI')
  assert.ok(html.includes("format('woff2')"), 'css names the woff2 container')
  assert.ok(!html.includes('data:font/woff;'), 'not the woff1 mime')
})
