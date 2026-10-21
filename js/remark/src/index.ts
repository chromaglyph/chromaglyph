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
import { readFileSync, writeFileSync } from 'node:fs'
import { isAbsolute, resolve } from 'node:path'
import { visit } from 'unist-util-visit'
import type { Code, Root } from 'mdast'
import type { Plugin } from 'unified'
import type { VFile } from 'vfile'
import { buildFontSafe, type BuildResult } from '@chromaglyph/node'
import { buildCssAssets, type CssAssets, type CssAssetsOptions } from '@chromaglyph/node/css'

export interface RemarkChromaglyphOptions {
  /** Path or buffer of the static TrueType source font (required). */
  font: string | Uint8Array
  /** Builtin theme names or paths to VS Code .json / .tmTheme files. */
  themes?: string[]
  /** Restrict processing to these languages; others keep default styling. */
  langs?: string[] | null
  /** Inline the font as a data: URI in the injected stylesheet (default true). */
  embed?: boolean
  /** Directory for emitted font files when `embed` is false. */
  outDir?: string
  /**
   * How the stylesheet reaches the page (default 'inline').
   * - 'inline': `<style>` node — plain static-HTML pipelines (Astro,
   *   Eleventy-style). Pipelines must keep raw html (remarkRehype +
   *   rehypeStringify with allowDangerousHtml, or rehype-raw).
   * - 'link': `<link rel="stylesheet">` node — required for pipelines that
   *   compile markdown into component templates: MDX/Docusaurus parses the
   *   output as JSX (CSS text with `{}` collides with JSX expressions),
   *   Vue-based pipelines reject `<style>` outright.
   */
  style?: 'inline' | 'link'
  /** Run the harfrust oracle (default true). */
  verify?: boolean
  /** Overlay tree-sitter semantic colors on the syntactic highlighting
   *  (parameters, variables-as-scoped, types, calls). Default false. */
  semantic?: boolean
  /** Preserve the source font's ligatures (`!=`, `->`, …) in exact mode:
   *  each substitution the font's own shaping performs is re-materialized
   *  as a rule, so ligatures render exactly as the font intends while
   *  staying highlighted. Sites spanning multiple colors degrade to plain
   *  per-char text (colors win). Default false. */
  ligatures?: boolean
  /** Output container: `'woff2'` (Brotli, smaller) or `'woff'` (zlib,
   *  default); both outline flavors ride in woff2. */
  format?: 'woff' | 'woff2'
  /** On-disk cache directory for incremental site builds; ignored when
   *  NODE_ENV=development (dev-server HMR would write one font per
   *  intermediate edit state). */
  cacheDir?: string
  /**
   * 'page' (default): all code blocks of one document batch into ONE font;
   * 'lang': one font per language — blocks carry `data-chromaglyph-lang`
   * and the CSS scopes each font to its own language's blocks. Editing one
   * language's snippet then rebuilds only that language's font, and
   * identical text under different languages can highlight each its own
   * way. Costs one subset base font (~10K) per extra language on a page.
   */
  grouping?: 'page' | 'lang'
}

/** One built font in per-language mode, for introspection via file.data. */
export interface ChromaglyphFontInfo {
  lang: string
  cached: boolean
  contentHash: string
  css: string
  fontUrl: string
  family: string
  warnings: string[]
}

/** Build introspection on `file.data.chromaglyph` after processing. */
export type ChromaglyphFileInfo =
  | {
      grouping?: undefined
      cached: boolean
      contentHash: string
      css: string
      fontUrl: string
      family?: string
      embed: boolean
      outDir: string | null
      warnings: string[]
      themes: { name: string; background?: string }[]
    }
  | {
      grouping: 'lang'
      fonts: ChromaglyphFontInfo[]
      css: string
      embed: boolean
      outDir: string | null
    }

interface PageFont {
  lang: string
  text: string
}

declare module 'vfile' {
  interface DataMap {
    chromaglyph?: ChromaglyphFileInfo
  }
}

const remarkChromaglyph: Plugin<[RemarkChromaglyphOptions?], Root> = (options) => {
  const {
    font,
    themes = ['dark-plus', 'light-plus'],
    langs = null,
    embed = true,
    outDir = 'chromaglyph-out',
    // 'inline' 注入 <style>（静态 HTML 管线，需 allowDangerousHtml）；'link'
    // 注入 <link rel="stylesheet">。MDX / Docusaurus 把产物编译成 JSX：CSS
    // 文本里的 {} 与 JSX 表达式语法相撞，<link>（data: URI 或落盘 css）无此
    // 问题；Vue 系管线则直接拒绝模板里的 <style>。
    style = 'inline',
    verify = true,
    semantic = false,
    // 保留源字体的连字(如 != -> ===):构建时用源字体 shaping,把每个
    // 替换重物化为规则——连字照常渲染且保持高亮;跨颜色的连字退化为
    // 逐字符(颜色优先)。
    ligatures = false,
    // 输出容器:woff2(Brotli,更小)或 woff(zlib);两种轮廓风格都能
    // 装进 woff2(TrueType 原样,CFF 走空变换)。
    format = 'woff',
    cacheDir,
    // 'page'（默认）：整篇文档合成一个字体；'lang'：按语言分组，一语言一
    // 字体（<pre> 带 data-chromaglyph-lang，CSS 按语言 scope）——改某语言
    // 的片段只重建该语言字体，不同语言的相同文本也可各自着色。
    grouping = 'page',
  } = options ?? {} as Partial<RemarkChromaglyphOptions>

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
  const scopeValue = (lang: string) => lang.replace(/[^a-zA-Z0-9_+#.-]/g, '') || 'x'

  // stylesheet node value for one build (inline <style>, data-URI <link>,
  // or a written-out css file + <link>); per-language mode concatenates
  // one node per font, the page mode emits exactly one
  const styleTag = (assets: CssAssets, cwd: string): string => {
    if (style === 'link') {
      if (embed) {
        return `<link rel="stylesheet" href="data:text/css;base64,${Buffer.from(assets.css).toString('base64')}">`
      }
      writeFileSync(resolve(cwd, outDir, `chromaglyph-${assets.hash8}.css`), assets.css)
      return `<link rel="stylesheet" href="./chromaglyph-${assets.hash8}.css">`
    }
    return `<style>\n${assets.css}</style>`
  }

  return async (tree: Root, file: VFile) => {
    const blocks: PageFont[] = []
    visit(tree, 'code', (node: Code) => {
      const lang = String(node.lang || 'plain').trim().split(/\s+/)[0] || 'plain'
      if (langs && !langs.includes(lang)) return
      const index = blocks.length
      blocks.push({ lang, text: String(node.value) })
      node.data = node.data || {}
      node.data.hProperties = node.data.hProperties || {}
      node.data.hProperties['data-chromaglyph'] = String(index)
      if (grouping === 'lang') {
        node.data.hProperties['data-chromaglyph-lang'] = scopeValue(lang)
      }
    })
    if (blocks.length === 0) return

    const cwd = file.cwd || '.'
    const fontBytes: Buffer = Buffer.isBuffer(font)
      ? font
      : readFileSync(isAbsolute(String(font)) ? String(font) : resolve(cwd, String(font)))

    // one build per language (first-appearance order), each scoped to its
    // own blocks; one failing language never takes the others down
    const build = (group: PageFont[], scope?: string) => {
      const result = buildFontSafe({
        font: fontBytes,
        themes,
        blocks: group.map((b, i) => ({ id: `block-${i}`, lang: b.lang, text: b.text })),
        verify,
        semantic,
        ligatures,
        format,
        cacheDir: diskCache,
      })
      if (!result) return null
      const assets = buildCssAssets(result, { embed, outDir, cwd: file.cwd || process.cwd(), ...(scope ? { lang: scope } : {}) } satisfies CssAssetsOptions)
      return { result, assets }
    }

    const inject = (value: string) => {
      tree.children.unshift({ type: 'html', value })
    }

    if (grouping === 'lang') {
      const byLang = new Map<string, PageFont[]>()
      for (const block of blocks) {
        if (!byLang.has(block.lang)) byLang.set(block.lang, [])
        byLang.get(block.lang)!.push(block)
      }
      const fonts: ChromaglyphFontInfo[] = []
      const assetsByFont: CssAssets[] = []
      for (const [lang, group] of byLang) {
        const made = build(group, scopeValue(lang))
        if (!made) continue
        const report = JSON.parse(made.result.report) as { warnings?: string[] }
        fonts.push({
          lang,
          cached: made.result.cached,
          contentHash: made.result.contentHash,
          css: made.assets.css,
          fontUrl: made.assets.fontUrl,
          family: made.assets.family,
          warnings: report.warnings || [],
        })
        assetsByFont.push(made.assets)
      }
      if (fonts.length === 0) return
      const css = fonts.map((f) => f.css).join('\n')
      file.data.chromaglyph = {
        grouping: 'lang',
        fonts,
        css,
        embed,
        outDir: embed ? null : outDir,
      }
      // inline mode keeps the historical single <style> with the joined
      // css; link mode carries one <link> per language (a data-URI
      // stylesheet per font, or one css file per font when embed: false)
      inject(
        style === 'inline'
          ? `<style>\n${css}</style>`
          : assetsByFont.map((a) => styleTag(a, cwd)).join('\n'),
      )
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
    const report = JSON.parse(result.report) as { warnings?: string[] }

    // inject the stylesheet as the document's first node; 'inline' mode
    // needs raw html downstream (remarkRehype + rehypeStringify with
    // allowDangerousHtml, or rehype-raw), 'link' is a plain element
    inject(styleTag(assets, cwd))

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
