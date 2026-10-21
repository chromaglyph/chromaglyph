/**
 * @chromaglyph/markdown-it — highlight code blocks by compiling them into a
 * chromaglyph color font. Works with plain markdown-it and VitePress
 * (which renders through markdown-it):
 *
 * ```js
 * // .vitepress/config.ts
 * import chromaglyph from '@chromaglyph/markdown-it'
 * export default {
 *   markdown: {
 *     config(md) {
 *       md.use(chromaglyph, { font: './JetBrainsMono-Regular.ttf', cacheDir: '.chromaglyph-cache' })
 *     },
 *   },
 * }
 * ```
 *
 * All fenced code blocks of one `md.render()` call are batched into ONE
 * font (`grouping: 'page'`, the default), or one font per language
 * (`grouping: 'lang'` — fences carry `data-chromaglyph-lang` and the CSS
 * scopes each font to its own language's blocks); `font-family` is scoped
 * to `[data-chromaglyph]` so prose is never affected; failures degrade
 * silently (no stylesheet, normal rendering); builds are content-hash
 * cached.
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { isAbsolute, resolve } from 'node:path'
import type MarkdownIt from 'markdown-it'
import type {
  Options as MarkdownItOptions,
  PluginWithOptions,
  Renderer,
  StateCore,
  Token,
} from 'markdown-it'
import { buildFontSafe, type BuildResult } from '@chromaglyph/node'
import { buildCssAssets, type CssAssets, type CssAssetsOptions } from '@chromaglyph/node/css'

export interface MarkdownItChromaglyphOptions {
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
   * - 'inline': `<style>` tag — plain markdown-it HTML pipelines.
   * - 'link': `<link rel="stylesheet">` — required for Vue-template
   *   frameworks (VitePress, Nuxt Content): a `<style>` inside a Vue
   *   component template is rejected by the client compiler and
   *   entity-escaped by SSR, so it never reaches the browser.
   */
  style?: 'inline' | 'link'
  /** Run the harfrust oracle (default true). */
  verify?: boolean
  /** Overlay tree-sitter semantic colors on the syntactic highlighting
   *  (parameters, variables-as-scoped, types, calls). Default false. */
  semantic?: boolean
  /** Preserve the source font's ligatures (`!=`, `->`, …) while
   *  highlighted; mixed-color sites degrade to per-char text (colors win).
   *  Default false. */
  ligatures?: boolean
  /** Output container: `'woff2'` (Brotli, smaller) or `'woff'` (zlib,
   *  default); both outline flavors ride in woff2. */
  format?: 'woff' | 'woff2'
  /** On-disk cache directory for incremental site builds; ignored when
   *  NODE_ENV=development (dev-server HMR would write one font per
   *  intermediate edit state). */
  cacheDir?: string
  /** Base directory for relative `font`/`outDir` paths (default: cwd). */
  cwd?: string
  /**
   * 'page' (default): all fences of one render batch into ONE font;
   * 'lang': one font per language — fences carry `data-chromaglyph-lang`
   * and the CSS scopes each font to its own language's blocks. Editing one
   * language's snippet then rebuilds only that language's font, and
   * identical text under different languages can highlight each its own
   * way. Costs one subset base font (~10K) per extra language on a page.
   */
  grouping?: 'page' | 'lang'
}

/** Per-render context the core rule produces and the fence rule consumes. */
interface ChromaglyphRenderContext {
  css: string | null
  fenceIndex: Map<Token, number>
  injected: boolean
}

type WithChromaglyph = MarkdownIt & {
  chromaglyph?: () => BuildResult | BuildResult[] | null
}

const markdownItChromaglyph: PluginWithOptions<MarkdownItChromaglyphOptions | undefined> = (
  md,
  options,
) => {
  const {
    font,
    themes = ['dark-plus', 'light-plus'],
    langs = null,
    embed = true,
    outDir = 'chromaglyph-out',
    // 'inline' 注入 <style>（纯 HTML 管线）；'link' 注入 <link rel="stylesheet">。
    // Vue 系框架（VitePress / Nuxt Content）把 markdown 产物当 Vue 模板编译：
    // <style> 在客户端编译器里是"副作用标签"（直接报错，dev 下页面 404），SSR
    // 又会把 CSS 文本当文本节点实体转义（@font-face 报废）。<link> 是普通元素，
    // 两条路都活——CSS 走 data: URI（embed:true）或落盘 css 文件（embed:false）。
    style = 'inline',
    verify = true,
    semantic = false,
    // 保留源字体的连字(如 != -> ===):同 remark 插件的 ligatures 选项。
    ligatures = false,
    // 输出容器:woff2(Brotli,更小)或 woff(zlib);两种轮廓风格都能
    // 装进 woff2(TrueType 原样,CFF 走空变换)。
    format = 'woff',
    cacheDir,
    cwd = process.cwd(),
    // 'page'（默认）：整页所有代码块合成一个字体；'lang'：按语言分组，一语言
    // 一字体（fence 带 data-chromaglyph-lang，CSS 按语言 scope）。改某语言的
    // 片段只重建该语言的字体，其余语言命中缓存；不同语言的相同文本也可各自
    // 着色。代价是每多一种语言多一份（子集化后 ~10K 的）基础字体。
    grouping = 'page',
  } = options ?? {} as Partial<MarkdownItChromaglyphOptions>

  if (!font) {
    throw new Error('markdownItChromaglyph: the `font` option (path or buffer of a static TTF) is required')
  }
  if (grouping !== 'page' && grouping !== 'lang') {
    throw new Error(`markdownItChromaglyph: unknown grouping '${grouping}' (expected 'page' or 'lang')`)
  }

  // The disk cache exists for cross-process builds (CI, `vitepress build`).
  // In a dev server every save is a new content state that will never be
  // requested again, so writing it grows the cache dir without bound —
  // HMR edits would leave one font per intermediate state. The in-memory
  // LRU still serves repeat renders inside the dev server process.
  const diskCache =
    cacheDir && process.env.NODE_ENV === 'development' ? undefined : cacheDir
  if (cacheDir && !diskCache) {
    console.info(
      'chromaglyph: NODE_ENV=development — disk cache disabled (cacheDir ignored); ' +
        'the in-memory cache still serves repeat renders',
    )
  }

  const loadFont = (): Buffer =>
    Buffer.isBuffer(font)
      ? font
      : readFileSync(isAbsolute(String(font)) ? String(font) : resolve(cwd, String(font)))

  // fence 语言要进 HTML 属性和 CSS 选择器,收窄到肯定安全的字符集
  const scopeValue = (lang: string) => lang.replace(/[^a-zA-Z0-9_+#.-]/g, '') || 'x'

  // stylesheet tag for one build (inline <style>, data-URI <link>, or a
  // written-out css file + <link>); per-language mode concatenates one
  // tag per font, the page mode emits exactly one
  const styleTag = (assets: CssAssets): string => {
    if (style === 'link') {
      if (embed) {
        return `<link rel="stylesheet" href="data:text/css;base64,${Buffer.from(assets.css).toString('base64')}">`
      }
      writeFileSync(resolve(cwd, outDir, `chromaglyph-${assets.hash8}.css`), assets.css)
      return `<link rel="stylesheet" href="./chromaglyph-${assets.hash8}.css">`
    }
    return `<style>\n${assets.css}</style>`
  }

  // last successful build(s): one BuildResult (page mode) or one per
  // language (lang mode), for tests/introspection after render finished
  let lastResult: BuildResult | null = null
  let lastResults: BuildResult[] | null = null

  // Collect the fences of one parse and build their font(s) in a core rule
  // at the end of the parse chain; the per-render context rides on env (a
  // fresh object per parse, so nested renders — frontmatter excerpts,
  // snippet includes — stay isolated for free). This must NOT be done by
  // replacing md.render with a from-scratch parse+render: host frameworks
  // assemble their markdown features as wrappers AROUND the render entry
  // points (VitePress's sfc plugin initializes env.sfcBlocks in its
  // md.render wrapper so html_block can hoist top-level <script setup>/
  // <style> out of the template; frontmatter wraps md.parse; headers and
  // title wrap renderer.render). A re-implemented render bypasses every
  // wrapper above it — the <script setup> then stayed inside the Vue
  // template and the client compiler rejected the page ("Tags with side
  // effect (<script> and <style>) are ignored in client component
  // templates"). A core rule runs inside every render path, however many
  // wrappers surround it.
  md.core.ruler.push('chromaglyph_fonts', (state: StateCore) => {
    const fenceIndex = new Map<Token, number>() // token -> block index
    const blocks: { id: string; lang: string; text: string }[] = []
    for (const token of state.tokens) {
      if (token.type !== 'fence' || token.hidden) continue
      const lang = String(token.info || '').trim().split(/\s+/)[0] || 'plain'
      if (langs && !langs.includes(lang)) continue
      fenceIndex.set(token, blocks.length)
      blocks.push({ id: `block-${blocks.length}`, lang, text: token.content })
    }

    let css: string | null = null
    let result: BuildResult | null = null
    let results: BuildResult[] | null = null
    if (blocks.length > 0) {
      // builds; silent degradation on failure (one font read per render,
      // shared by every language group)
      const fontBytes = loadFont()
      const emit = (group: { lang: string; text: string }[], scope?: string) => {
        const built = buildFontSafe({
          font: fontBytes,
          themes,
          blocks: group.map((b, i) => ({ id: `block-${i}`, lang: b.lang, text: b.text })),
          verify,
          semantic,
          ligatures,
          format,
          cacheDir: diskCache,
        })
        if (!built) return null
        const assets = buildCssAssets(built, { embed, outDir, cwd, ...(scope ? { lang: scope } : {}) } satisfies CssAssetsOptions)
        return { built, tag: styleTag(assets) }
      }

      if (grouping === 'lang') {
        // group by language in first-appearance order; plain blocks form
        // their own zero-rule group; one failing language never takes the
        // others down
        const byLang = new Map<string, typeof blocks>()
        for (const block of blocks) {
          if (!byLang.has(block.lang)) byLang.set(block.lang, [])
          byLang.get(block.lang)!.push(block)
        }
        results = []
        const tags: string[] = []
        for (const [lang, group] of byLang) {
          const made = emit(
            group.map((b, i) => ({ id: `block-${i}`, lang: b.lang, text: b.text })),
            scopeValue(lang),
          )
          if (!made) continue
          results.push(made.built)
          tags.push(made.tag)
        }
        if (results.length === 0) results = null
        if (tags.length > 0) css = tags.join('\n')
      } else {
        const made = emit(blocks)
        if (made) {
          result = made.built
          css = made.tag
        }
      }
    }
    lastResult = result
    lastResults = results

    state.env.chromaglyph = { css, fenceIndex, injected: false } satisfies ChromaglyphRenderContext
  })

  // chain onto whatever fence rule is installed (VitePress ships its own)
  const prevFence = md.renderer.rules.fence
  md.renderer.rules.fence = (
    tokens: Token[],
    idx: number,
    opts: MarkdownItOptions,
    env: { chromaglyph?: ChromaglyphRenderContext } | undefined,
    self: Renderer,
  ) => {
    const base = prevFence
      ? prevFence(tokens, idx, opts, env, self)
      : self.renderToken(tokens, idx, opts)
    const current = env && env.chromaglyph
    if (!current) return base
    const token = tokens[idx]
    if (!token || !current.fenceIndex.has(token)) return base
    const index = current.fenceIndex.get(token)!
    // v-pre：Vue 系框架按模板编译 markdown 产物，代码里的 {{ }} 会被当成
    // 插值；纯 HTML 管线里它只是一个无害的自定义属性。lang 分组模式下
    // 额外携带语言标记，CSS 据此把每语言的字体 scope 到自己的代码块
    const scopeAttr =
      grouping === 'lang'
        ? ` data-chromaglyph-lang="${scopeValue(String(token.info || '').trim().split(/\s+/)[0] || 'plain')}"`
        : ''
    let out = base.includes('<pre')
      ? base.replace('<pre', `<pre v-pre data-chromaglyph="${index}"${scopeAttr}`)
      : base
    if (!current.injected && current.css) {
      current.injected = true
      out = `${current.css}\n${out}`
    }
    return out
  }

  // expose the last successful build result(s) for tests/introspection
  ;(md as WithChromaglyph).chromaglyph = () => (grouping === 'lang' ? lastResults : lastResult)
}

export { markdownItChromaglyph }
export default markdownItChromaglyph
