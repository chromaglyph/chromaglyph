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
 * font; `font-family` is scoped to `[data-chromaglyph]` so prose is never
 * affected; failures degrade silently (no stylesheet, normal rendering);
 * builds are content-hash cached.
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
 *  cacheDir?: string,
 *  cwd?: string,
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
    cacheDir,
    cwd = process.cwd(),
  } = options

  if (!font) {
    throw new Error('markdownItChromaglyph: the `font` option (path or buffer of a static TTF) is required')
  }

  const loadFont = () =>
    Buffer.isBuffer(font)
      ? font
      : readFileSync(isAbsolute(String(font)) ? String(font) : resolve(cwd, String(font)))

  // per-render context consumed by the fence renderer below; saved and
  // restored so nested md.render calls (e.g. snippet includes) stay isolated
  let current = null
  // last successful build, for tests/introspection after render finished
  let lastResult = null

  const origRender = md.render.bind(md)
  md.render = (src, env) => {
    // phase 1: parse once (this is exactly what md.render does internally)
    const tokens = md.parse(src, env)
    const fenceIndex = new Map() // token -> block index
    const blocks = []
    for (const token of tokens) {
      if (token.type !== 'fence' || token.hidden) continue
      const lang = String(token.info || '').trim().split(/\s+/)[0] || 'plain'
      if (langs && !langs.includes(lang)) continue
      fenceIndex.set(token, blocks.length)
      blocks.push({ id: `block-${blocks.length}`, lang, text: token.content })
    }

    // phase 2: one batched build; silent degradation on failure
    let css = null
    let result = null
    if (blocks.length > 0) {
      result = buildFontSafe({ font: loadFont(), themes, blocks, verify, semantic, cacheDir })
      if (result) {
        const assets = buildCssAssets(result, { embed, outDir, cwd })
        if (style === 'link') {
          if (embed) {
            css = `<link rel="stylesheet" href="data:text/css;base64,${Buffer.from(assets.css).toString('base64')}">`
          } else {
            writeFileSync(resolve(cwd, outDir, `chromaglyph-${assets.hash8}.css`), assets.css)
            css = `<link rel="stylesheet" href="./chromaglyph-${assets.hash8}.css">`
          }
        } else {
          css = `<style>\n${assets.css}</style>`
        }
        lastResult = result
      }
    }

    // phase 3: render from the already-parsed tokens with our fence rule
    const previous = current
    current = { css, result, fenceIndex, injected: false }
    try {
      return md.renderer.render(tokens, md.options, env)
    } finally {
      current = previous
    }
  }

  // chain onto whatever fence rule is installed (VitePress ships its own)
  const prevFence = md.renderer.rules.fence
  md.renderer.rules.fence = (tokens, idx, opts, env, self) => {
    const base = prevFence
      ? prevFence(tokens, idx, opts, env, self)
      : self.renderToken(tokens, idx, opts)
    if (!current) return base
    const token = tokens[idx]
    if (!current.fenceIndex.has(token)) return base
    const index = current.fenceIndex.get(token)
    // v-pre：Vue 系框架按模板编译 markdown 产物，代码里的 {{ }} 会被当成
    // 插值；纯 HTML 管线里它只是一个无害的自定义属性
    let out = base.includes('<pre')
      ? base.replace('<pre', `<pre v-pre data-chromaglyph="${index}"`)
      : base
    if (!current.injected && current.css) {
      current.injected = true
      out = `${current.css}\n${out}`
    }
    return out
  }

  // expose the last successful build result for tests/introspection
  md.chromaglyph = () => lastResult
}

export { markdownItChromaglyph }
export default markdownItChromaglyph
