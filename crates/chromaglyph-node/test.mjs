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
  assert.ok(report.rules >= 300, `real rule machine, got ${report.rules}`)
  assert.ok(report.verify.ok, 'oracle green')
  assert.equal(report.palettes.length, 2, 'one palette per theme')
  assert.ok(out.css.includes('@font-palette-values'), 'css companion present')
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
