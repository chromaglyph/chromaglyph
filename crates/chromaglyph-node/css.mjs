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
  // scope to the attribute the plugins add to highlighted <pre>; `code` is
  // included because site CSS often sets font-family on code directly
  css += `[data-chromaglyph], [data-chromaglyph] code {\n  font-family: ${family}, monospace;\n`
  if (dark && dark.background) css += `  background: ${dark.background};\n`
  css += `}\n`
  if (light) {
    css += `@media (prefers-color-scheme: light) {\n  [data-chromaglyph], [data-chromaglyph] code {\n    font-palette: --${family}-${slug(light.name)};\n`
    if (light.background) css += `    background: ${light.background};\n`
    css += `  }\n}\n`
  }
  return { css, fontUrl, family, hash8 }
}
