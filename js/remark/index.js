/**
 * @chromaglyph/remark — highlight code blocks by compiling them into a
 * chromaglyph color font.
 *
 * All fenced code blocks of a document are batched into ONE font (global
 * disambiguation across languages is handled by the engine). The generated
 * CSS scopes `font-family` to `[data-chromaglyph]`, so prose and code the
 * font was not built for are never affected. Builds are content-hash cached
 * (in-process + optional on-disk), and any failure degrades silently: the
 * document renders without font-based highlighting instead of breaking the
 * site build.
 *
 * ```js
 * unified().use(remarkParse).use(remarkChromaglyph, { font: './JBM.ttf' })
 *   .use(remarkRehype, { allowDangerousHtml: true })
 *   .use(rehypeStringify, { allowDangerousHtml: true })
 * ```
 */
import { readFileSync } from 'node:fs'
import { isAbsolute, resolve } from 'node:path'
import { mkdirSync, writeFileSync } from 'node:fs'
import { visit } from 'unist-util-visit'
import { buildFontSafe } from '@chromaglyph/node'

const slug = (name) =>
  name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')

/** @typedef {import('node:fs').PathOrFileDescriptor} PathLike */

/**
 * @param {{
 *   font: PathLike | Uint8Array,
 *   themes?: string[],
 *   langs?: string[] | null,
 *   embed?: boolean,
 *   outDir?: string,
 *   verify?: boolean,
 *   cacheDir?: string,
 * }} [options]
 */
function remarkChromaglyph(options = {}) {
  const {
    font,
    themes = ['dark-plus', 'light-plus'],
    langs = null,
    embed = true,
    outDir = 'chromaglyph-out',
    verify = true,
    cacheDir,
  } = options

  if (!font) {
    throw new Error('remarkChromaglyph: the `font` option (path or buffer of a static TTF) is required')
  }

  return async (tree, file) => {
    const blocks = []
    visit(tree, 'code', (node) => {
      const lang = String(node.lang || 'plain').trim().split(/\s+/)[0] || 'plain'
      if (langs && !langs.includes(lang)) return
      const index = blocks.length
      blocks.push({ id: `block-${index}`, lang, text: String(node.value) })
      node.data = node.data || {}
      node.data.hProperties = node.data.hProperties || {}
      node.data.hProperties['data-chromaglyph'] = String(index)
    })
    if (blocks.length === 0) return

    const fontBytes = Buffer.isBuffer(font)
      ? font
      : readFileSync(isAbsolute(String(font)) ? String(font) : resolve(file.cwd || '.', String(font)))

    // silent degradation: any failure returns null and the document renders
    // with its usual (uncolored) code styling
    const result = buildFontSafe({ font: fontBytes, themes, blocks, verify, cacheDir })
    if (!result) {
      file.message('chromaglyph: font build degraded silently (see stderr for nothing — it is silent)')
      return
    }

    const hash8 = result.contentHash.slice(0, 8)
    const family = `chromaglyph-${hash8}`
    let fontUrl
    if (embed) {
      fontUrl = `data:font/woff;base64,${result.woff.toString('base64')}`
    } else {
      mkdirSync(outDir, { recursive: true })
      const fileName = `chromaglyph-${hash8}.woff`
      writeFileSync(resolve(outDir, fileName), result.woff)
      fontUrl = `./${fileName}`
    }

    let css = `/* chromaglyph ${hash8} */\n`
    css += `@font-face {\n  font-family: '${family}';\n  src: url(${fontUrl}) format('woff');\n}\n`
    for (const [i, theme] of result.themes.entries()) {
      css += `@font-palette-values --${family}-${slug(theme.name)} {\n  font-family: ${family};\n  base-palette: ${i};\n}\n`
    }
    const dark = result.themes[0]
    const light = result.themes[1]
    css += `[data-chromaglyph] {\n  font-family: ${family}, monospace;\n`
    if (dark && dark.background) css += `  background: ${dark.background};\n`
    css += `}\n`
    if (light) {
      css += `@media (prefers-color-scheme: light) {\n  [data-chromaglyph] {\n    font-palette: --${family}-${slug(light.name)};\n`
      if (light.background) css += `    background: ${light.background};\n`
      css += `  }\n}\n`
    }

    // inject the stylesheet as the document's first node; pipelines must
    // keep raw html (remarkRehype + rehypeStringify with allowDangerousHtml,
    // or rehype-raw)
    tree.children.unshift({ type: 'html', value: `<style>\n${css}</style>` })

    const report = JSON.parse(result.report)
    file.data.chromaglyph = {
      cached: result.cached,
      contentHash: result.contentHash,
      css,
      fontUrl,
      embed,
      outDir: embed ? null : outDir,
      warnings: report.warnings || [],
      themes: result.themes,
    }
    for (const warning of report.warnings || []) {
      file.message(`chromaglyph: ${warning}`)
    }
  }
}

export { remarkChromaglyph }
export default remarkChromaglyph
