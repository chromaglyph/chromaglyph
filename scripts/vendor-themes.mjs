#!/usr/bin/env node
/**
 * vendor-themes.mjs — copy shiki's `tm-themes` collection into
 * crates/chromaglyph-core/themes/tm/ as the builtin theme set.
 *
 * The pinned devDependency is the source of truth; `pnpm up tm-themes` then
 * re-running this script is the whole update procedure. Theme files are
 * copied byte-for-byte (so diffs against upstream stay reviewable and the
 * license headers inside them survive); the script only decides WHICH files
 * ship and writes two derived files next to them:
 *
 *   index.json  registry the core's build.rs embeds (name, type, license,
 *               provenance) — the order of builtin themes everywhere
 *   NOTICE      upstream third-party notices, filtered to the shipped files
 *
 * Themes are embedded into an Apache-2.0 binary, so only permissive licenses
 * ship; anything else is skipped and listed. CI runs this script and fails on
 * a dirty tree, which keeps the vendored copy in lockstep with the lockfile.
 *
 * Usage: node scripts/vendor-themes.mjs
 */
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { themes } from 'tm-themes'

const ALLOWED_LICENSES = new Set(['MIT', 'Apache-2.0'])

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
// the package exports only its entry and `themes/*`, so locate it by the entry
const src = dirname(fileURLToPath(import.meta.resolve('tm-themes')))
const version = JSON.parse(readFileSync(join(src, 'package.json'), 'utf8')).version
const dest = join(root, 'crates/chromaglyph-core/themes/tm')

mkdirSync(dest, { recursive: true })
for (const f of readdirSync(dest)) {
  if (f.endsWith('.json') || f === 'NOTICE') rmSync(join(dest, f))
}

const shipped = []
const skipped = []
for (const t of [...themes].sort((a, b) => a.name.localeCompare(b.name))) {
  if (!ALLOWED_LICENSES.has(t.license)) {
    skipped.push(`${t.name} (${t.license ?? 'no license'})`)
    continue
  }
  const file = `${t.name}.json`
  const bytes = readFileSync(join(src, 'themes', file))
  const parsed = JSON.parse(bytes)
  // builtins are embedded one file each: an include would have nothing to
  // resolve against, so refuse here rather than degrade silently at runtime
  if (parsed.include) throw new Error(`${file}: has an "include" — flatten upstream first`)
  if (parsed.type !== 'dark' && parsed.type !== 'light') {
    throw new Error(`${file}: type is ${JSON.stringify(parsed.type)}, expected dark|light`)
  }
  writeFileSync(join(dest, file), bytes)
  shipped.push({
    name: t.name,
    displayName: t.displayName,
    type: t.type,
    license: t.license,
    licenseUrl: t.licenseUrl,
    source: t.source,
  })
}

writeFileSync(
  join(dest, 'index.json'),
  JSON.stringify({ 'tm-themes': version, themes: shipped }, null, 2) + '\n',
)

// upstream NOTICE: blocks separated by `====` rules, each opening with
// `Files: a.json, b.json`; keep the blocks that cover a shipped file
const shippedFiles = new Set(shipped.map((t) => `${t.name}.json`))
const notice = readFileSync(join(src, 'NOTICE'), 'utf8')
const [preamble, ...blocks] = notice.split(/^=+\n/m)
const kept = blocks.filter((block) => {
  const m = block.match(/^Files:\s*(.*)$/m)
  return m && m[1].split(',').some((f) => shippedFiles.has(f.trim()))
})
const rule = '='.repeat(105) + '\n'
writeFileSync(
  join(dest, 'NOTICE'),
  `Builtin themes vendored from tm-themes ${version} (https://github.com/shikijs/textmate-grammars-themes),\n` +
    `filtered to the ${shipped.length} themes chromaglyph ships. Upstream notice follows.\n\n` +
    preamble +
    kept.map((b) => rule + b).join(''),
)

console.log(`tm-themes ${version}: vendored ${shipped.length} themes into ${dest}`)
if (skipped.length) console.log(`skipped (license not in allowlist): ${skipped.join(', ')}`)
