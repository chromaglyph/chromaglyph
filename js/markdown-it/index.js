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
import { buildFontSafe } from '@chromaglyph/node'
import { buildCssAssets } from '@chromaglyph/node/css'

/**
 * @param {import('markdown-it').MarkdownIt} md
 * @param {{
 *  font: string | Uint8Array,
 *  themes?: string[],
 *  langs?: string[] | null,
 *  embed?: boolean,
 *  outDir?: string,
 *  style?: 'inline' | 'link',
 *  verify?: boolean,
 *  semantic?: boolean,
 *  ligatures?: boolean,
 *  format?: 'woff' | 'woff2',
 *  cacheDir?: string,   // on-disk cache for cross-process builds; ignored when NODE_ENV=development
 *  cwd?: string,
 *  grouping?: 'page' | 'lang',
 * }} [options]
 */
function markdownItChromaglyph(md, options = {}) {
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
  } = options

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

  const loadFont = () =>
    Buffer.isBuffer(font)
      ? font
      : readFileSync(isAbsolute(String(font)) ? String(font) : resolve(cwd, String(font)))

  // fence 语言要进 HTML 属性和 CSS 选择器,收窄到肯定安全的字符集
  const scopeValue = (lang) => lang.replace(/[^a-zA-Z0-9_+#.-]/g, '') || 'x'

  // stylesheet tag for one build (inline <style>, data-URI <link>, or a
  // written-out css file + <link>); per-language mode concatenates one
  // tag per font, the page mode emits exactly one
  const styleTag = (assets) => {
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
  let lastResult = null
  let lastResults = null

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
  md.core.ruler.push('chromaglyph_fonts', (state) => {
    const fenceIndex = new Map() // token -> block index
    const blocks = []
    for (const token of state.tokens) {
      if (token.type !== 'fence' || token.hidden) continue
      const lang = String(token.info || '').trim().split(/\s+/)[0] || 'plain'
      if (langs && !langs.includes(lang)) continue
      fenceIndex.set(token, blocks.length)
      blocks.push({ id: `block-${blocks.length}`, lang, text: token.content })
    }

    let css = null
    let result = null
    let results = null
    if (blocks.length > 0) {
      // builds; silent degradation on failure (one font read per render,
      // shared by every language group)
      const fontBytes = loadFont()
      const emit = (group, scope) => {
        const built = buildFontSafe({ font: fontBytes, themes, blocks: group, verify, semantic, ligatures, format, cacheDir: diskCache })
        if (!built) return null
        const assets = buildCssAssets(built, { embed, outDir, cwd, ...(scope ? { lang: scope } : {}) })
        return { built, tag: styleTag(assets) }
      }

      if (grouping === 'lang') {
        // group by language in first-appearance order; plain blocks form
        // their own zero-rule group; one failing language never takes the
        // others down
        const byLang = new Map()
        for (const block of blocks) {
          if (!byLang.has(block.lang)) byLang.set(block.lang, [])
          byLang.get(block.lang).push(block)
        }
        results = []
        const tags = []
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

    state.env.chromaglyph = { css, fenceIndex, injected: false }
  })

  // chain onto whatever fence rule is installed (VitePress ships its own)
  const prevFence = md.renderer.rules.fence
  md.renderer.rules.fence = (tokens, idx, opts, env, self) => {
    const base = prevFence
      ? prevFence(tokens, idx, opts, env, self)
      : self.renderToken(tokens, idx, opts)
    const current = env && env.chromaglyph
    if (!current) return base
    const token = tokens[idx]
    if (!current.fenceIndex.has(token)) return base
    const index = current.fenceIndex.get(token)
    // v-pre：Vue 系框架按模板编译 markdown 产物，代码里的 {{ }} 会被当成
    // 插值；纯 HTML 管线里它只是一个无害的自定义属性。lang 分组模式下
    // 额外携带语言标记，CSS 据此把每语言的字体 scope 到自己的代码块
    const scopeAttr = grouping === 'lang' ? ` data-chromaglyph-lang="${scopeValue(String(token.info || '').trim().split(/\s+/)[0] || 'plain')}"` : ''
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
  md.chromaglyph = () => (grouping === 'lang' ? lastResults : lastResult)
}

export { markdownItChromaglyph }
export default markdownItChromaglyph
