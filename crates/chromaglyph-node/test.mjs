import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { buildFont, buildFontSafe, clearCache } from './index.js'

const FONT = readFileSync('../chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf')

test('generic mode builds a font that highlights any text', () => {
  clearCache()
  const out = buildFont({
    font: FONT,
    themes: ['dark-plus', 'light-plus'],
    blocks: [],
    generic: 'js',
  })
  assert.ok(out.woff.length > 50_000, 'a real font comes back')
  const report = JSON.parse(out.report)
  assert.ok(report.rules >= 250, `real rule machine, got ${report.rules}`)
  assert.ok(report.verify.ok, 'oracle green')
  assert.equal(report.palettes.length, 2, 'one palette per theme')
  assert.ok(out.css.includes('@font-palette-values'), 'css companion present')
  assert.ok(out.css.includes('--chromaglyph-dark-palette: --chromaglyph-'), 'hash-free palette alias')
  assert.ok(out.css.includes('font-palette: var(--chromaglyph-palette,'), 'palette is var-driven')
  assert.ok(out.css.includes('background: var(--chromaglyph-bg,'), 'background is var-driven')
})

test('generic language is part of the content hash', () => {
  clearCache()
  const base = { font: FONT, themes: ['dark-plus'], blocks: [], verify: false }
  const js = buildFont({ ...base, generic: 'js' })
  const py = buildFont({ ...base, generic: 'python' })
  assert.notEqual(js.contentHash, py.contentHash, 'different languages differ')
  const again = buildFont({ ...base, generic: 'js' })
  assert.ok(again.cached, 'identical inputs hit the cache')
  assert.equal(again.contentHash, js.contentHash)
})

test('unknown generic language degrades safely / throws loudly', () => {
  clearCache()
  assert.equal(buildFontSafe({ font: FONT, themes: ['dark-plus'], blocks: [], generic: 'cobol' }), null)
  assert.throws(() => buildFont({ font: FONT, themes: ['dark-plus'], blocks: [], generic: 'cobol' }))
})

test('format woff2 ships a wOF2 container and its own cache entry', () => {
  clearCache()
  const base = {
    font: FONT,
    themes: ['dark-plus', 'light-plus'],
    blocks: [{ id: 'b0', lang: 'rust', text: 'let x = "hi"; // c\n' }],
  }
  const w1 = buildFont(base)
  const w2 = buildFont({ ...base, format: 'woff2' })
  // the container follows the option: wOF2 magic, and smaller bytes
  assert.equal(Buffer.from(w1.woff).subarray(0, 4).toString('latin1'), 'wOFF')
  assert.equal(Buffer.from(w2.woff).subarray(0, 4).toString('latin1'), 'wOF2')
  assert.ok(w2.woff.length < w1.woff.length, `woff2 ${w2.woff.length} < woff1 ${w1.woff.length}`)
  // format is part of the content hash: no cross-container cache hits
  assert.notEqual(w1.contentHash, w2.contentHash)
  assert.ok(buildFont({ ...base, format: 'woff2' }).cached, 'same format hits the cache')
  assert.ok(buildFont(base).cached, 'and so does the woff1 variant again')
  assert.throws(() => buildFont({ ...base, format: 'eot' }), /unknown format/)
})

test('CFF-flavored woff2 request falls back to woff1 with a warning', () => {
  clearCache()
  const otf = readFileSync('../chromaglyph-core/tests/fixtures/SourceCodePro-Regular.otf')
  const out = buildFont({
    font: otf,
    themes: ['dark-plus'],
    blocks: [{ id: 'b0', lang: 'rust', text: 'let x = "hi"; // c\n' }],
    format: 'woff2',
  })
  assert.equal(Buffer.from(out.woff).subarray(0, 4).toString('latin1'), 'wOFF')
  const report = JSON.parse(out.report)
  assert.ok(
    report.warnings.some((w) => w.includes('WOFF1') && w.includes('WOFF2')),
    JSON.stringify(report.warnings),
  )
})
