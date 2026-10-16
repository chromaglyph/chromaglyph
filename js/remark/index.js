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
import { visit } from 'unist-util-visit'
import { buildFontSafe } from '@chromaglyph/node'
import { buildCssAssets } from '@chromaglyph/node/css'

/**
 * @param {{
 *   font: string | Uint8Array,
 *   themes?: string[],
 *   langs?: string[] | null,
 *   embed?: boolean,
 *   outDir?: string,
 *   verify?: boolean,
 *   semantic?: boolean,
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
    semantic = false,
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
    const result = buildFontSafe({ font: fontBytes, themes, blocks, verify, semantic, cacheDir })
    if (!result) return

    const assets = buildCssAssets(result, { embed, outDir, cwd: file.cwd || process.cwd() })

    // inject the stylesheet as the document's first node; pipelines must
    // keep raw html (remarkRehype + rehypeStringify with allowDangerousHtml,
    // or rehype-raw)
    tree.children.unshift({ type: 'html', value: `<style>\n${assets.css}</style>` })

    const report = JSON.parse(result.report)
    file.data.chromaglyph = {
      cached: result.cached,
      contentHash: result.contentHash,
      css: assets.css,
      fontUrl: assets.fontUrl,
      family: assets.family,
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
