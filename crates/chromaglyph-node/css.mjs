// Shared CSS/asset builder for JS-side plugins (remark, markdown-it, …).
// Keeps the plugins byte-identical in how they scope the font:
// [data-chromaglyph] (the attribute plugins put on rendered <pre>) and
// nothing else — prose can never be touched by the compiled rules.
import { mkdirSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

const slug = (name) =>
  name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')

/**
 * Build the companion CSS (and optional font file) for a chromaglyph build
 * result. Returns `{ css, fontUrl, family, hash8 }`.
 *
 * @param {{ woff: Uint8Array, contentHash: string, themes: {name: string, background?: string}[] }} result
 * @param {{ embed?: boolean, outDir?: string, cwd?: string }} [options]
 */
export function buildCssAssets(result, options = {}) {
  const { embed = true, outDir = 'chromaglyph-out', cwd = process.cwd() } = options
  const hash8 = result.contentHash.slice(0, 8)
  const family = `chromaglyph-${hash8}`

  let fontUrl
  if (embed) {
    fontUrl = `data:font/woff;base64,${Buffer.from(result.woff).toString('base64')}`
  } else {
    mkdirSync(resolve(cwd, outDir), { recursive: true })
    const fileName = `chromaglyph-${hash8}.woff`
    writeFileSync(resolve(cwd, outDir, fileName), result.woff)
    fontUrl = `./${fileName}`
  }

  let css = `/* chromaglyph ${hash8} */\n`
  css += `@font-face {\n  font-family: '${family}';\n  src: url(${fontUrl}) format('woff');\n}\n`
  for (const [i, theme] of result.themes.entries()) {
    css += `@font-palette-values --${family}-${slug(theme.name)} {\n  font-family: ${family};\n  base-palette: ${i};\n}\n`
  }
  const dark = result.themes[0]
  const light = result.themes[1]
  // hash-free aliases for the first (dark) / second (light) theme. Sites
  // drive palette + background through --chromaglyph-palette /
  // --chromaglyph-bg below and never touch the hash-derived names; the
  // aliases live in the same per-page stylesheet as the font they point
  // at, so SPA page swaps stay correct with zero runtime JS
  css += `:root {\n  --chromaglyph-dark-palette: --${family}-${slug(dark.name)};\n`
  if (light) css += `  --chromaglyph-light-palette: --${family}-${slug(light.name)};\n`
  css += `}\n`
  // scope to the attribute the plugins add to highlighted <pre>; `code` is
  // included because site CSS often sets font-family on code directly.
  // The var() indirection is the class-toggle escape hatch: a site that
  // defines --chromaglyph-palette (e.g. per html.dark) overrides BOTH the
  // dark default here and the prefers-color-scheme branch below in one
  // place; --chromaglyph-bg detaches from the theme-baked background
  css += `[data-chromaglyph], [data-chromaglyph] code {\n  font-family: ${family}, monospace;\n`
  css += `  font-palette: var(--chromaglyph-palette, var(--chromaglyph-dark-palette));\n`
  if (dark && dark.background) css += `  background: var(--chromaglyph-bg, ${dark.background});\n`
  css += `}\n`
  if (light) {
    css += `@media (prefers-color-scheme: light) {\n  [data-chromaglyph], [data-chromaglyph] code {\n`
    css += `    font-palette: var(--chromaglyph-palette, var(--chromaglyph-light-palette));\n`
    if (light.background) css += `    background: var(--chromaglyph-bg, ${light.background});\n`
    css += `  }\n}\n`
  }
  return { css, fontUrl, family, hash8 }
}
