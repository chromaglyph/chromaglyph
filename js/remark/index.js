/**
 * @chromaglyph/remark — highlight code blocks by compiling them into a
 * chromaglyph color font.
 *
 * All fenced code blocks of a document are batched into ONE font
 * (`grouping: 'page'`, the default) with global disambiguation handled by
 * the engine, or one font per language (`grouping: 'lang'` — code blocks
 * carry `data-chromaglyph-lang` and the CSS scopes each font to its own
 * language). The generated CSS scopes `font-family` to `[data-chromaglyph]`,
 * so prose and code the font was not built for are never affected. Builds
 * are content-hash cached (in-process + optional on-disk), and any failure
 * degrades silently: the document renders without font-based highlighting
 * instead of breaking the site build.
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
 *   ligatures?: boolean,
 *   cacheDir?: string, // on-disk cache for cross-process builds; ignored when NODE_ENV=development
 *   grouping?: 'page' | 'lang',
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
    // 保留源字体的连字(如 != -> ===):构建时用源字体 shaping,把每个
    // 替换重物化为规则——连字照常渲染且保持高亮;跨颜色的连字退化为
    // 逐字符(颜色优先)。
    ligatures = false,
    cacheDir,
    // 'page'（默认）：整篇文档合成一个字体；'lang'：按语言分组，一语言一
    // 字体（<pre> 带 data-chromaglyph-lang，CSS 按语言 scope）——改某语言
    // 的片段只重建该语言字体，不同语言的相同文本也可各自着色。
    grouping = 'page',
  } = options

  if (!font) {
    throw new Error('remarkChromaglyph: the `font` option (path or buffer of a static TTF) is required')
  }
  if (grouping !== 'page' && grouping !== 'lang') {
    throw new Error(`remarkChromaglyph: unknown grouping '${grouping}' (expected 'page' or 'lang')`)
  }

  // The disk cache exists for cross-process builds (CI, static site
  // builds). In a dev server every save is a new content state that will
  // never be requested again, so writing it grows the cache dir without
  // bound. The in-memory LRU still serves repeat renders in-process.
  const diskCache =
    cacheDir && process.env.NODE_ENV === 'development' ? undefined : cacheDir
  if (cacheDir && !diskCache) {
    console.info(
      'chromaglyph: NODE_ENV=development — disk cache disabled (cacheDir ignored); ' +
        'the in-memory cache still serves repeat renders',
    )
  }

  // fence 语言要进 HTML 属性和 CSS 选择器,收窄到肯定安全的字符集
  const scopeValue = (lang) => lang.replace(/[^a-zA-Z0-9_+#.-]/g, '') || 'x'

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
      if (grouping === 'lang') {
        node.data.hProperties['data-chromaglyph-lang'] = scopeValue(lang)
      }
    })
    if (blocks.length === 0) return

    const fontBytes = Buffer.isBuffer(font)
      ? font
      : readFileSync(isAbsolute(String(font)) ? String(font) : resolve(file.cwd || '.', String(font)))

    // one build per language (first-appearance order), each scoped to its
    // own blocks; one failing language never takes the others down
    const build = (group, scope) => {
      const result = buildFontSafe({
        font: fontBytes,
        themes,
        blocks: group.map((b, i) => ({ id: `block-${i}`, lang: b.lang, text: b.text })),
        verify,
        semantic,
        ligatures,
        cacheDir: diskCache,
      })
      if (!result) return null
      return { result, assets: buildCssAssets(result, { embed, outDir, cwd: file.cwd || process.cwd(), ...(scope ? { lang: scope } : {}) }) }
    }

    if (grouping === 'lang') {
      const byLang = new Map()
      for (const block of blocks) {
        if (!byLang.has(block.lang)) byLang.set(block.lang, [])
        byLang.get(block.lang).push(block)
      }
      const fonts = []
      for (const [lang, group] of byLang) {
        const made = build(group, scopeValue(lang))
        if (!made) continue
        const report = JSON.parse(made.result.report)
        fonts.push({
          lang,
          cached: made.result.cached,
          contentHash: made.result.contentHash,
          css: made.assets.css,
          fontUrl: made.assets.fontUrl,
          family: made.assets.family,
          warnings: report.warnings || [],
        })
      }
      if (fonts.length === 0) return
      const css = fonts.map((f) => f.css).join('\n')
      tree.children.unshift({ type: 'html', value: `<style>\n${css}</style>` })
      file.data.chromaglyph = {
        grouping: 'lang',
        fonts,
        css,
        embed,
        outDir: embed ? null : outDir,
      }
      for (const f of fonts) {
        for (const warning of f.warnings) {
          file.message(`chromaglyph: ${warning}`)
        }
      }
      return
    }

    // silent degradation: any failure returns null and the document renders
    // with its usual (uncolored) code styling
    const made = build(blocks)
    if (!made) return
    const { result, assets } = made

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
