// Generates themes.data.json for the Themes page: asks the native binding
// for the builtin registry (build-time Node, tens of ms) and writes it as
// plain JSON that pages/components import. Run by docs:build before
// vitepress — VitePress 2.0-alpha has no data loaders yet.
import { readFileSync, writeFileSync } from 'node:fs'
import { listThemes, tmThemesVersion } from '@chromaglyph/node'

const out = new URL('../themes.data.json', import.meta.url)
writeFileSync(out, JSON.stringify({ version: tmThemesVersion(), themes: listThemes() }))
console.log(`themes.data.json: ${JSON.parse(readFileSync(out)).themes.length} themes`)
