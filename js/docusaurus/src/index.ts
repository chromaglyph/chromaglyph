/**
 * @chromaglyph/docusaurus — chromaglyph syntax highlighting for Docusaurus.
 *
 * Docusaurus renders markdown/MDX through the unified pipeline
 * (`markdown.remarkPlugins` / `markdown.rehypePlugins` in
 * `docusaurus.config`), and MDX compiles the output to JSX — which rules
 * out both stylesheet shapes a plain remark pipeline would use:
 *
 * - an injected `html` node dies in MDX's hast→estree step ("Cannot
 *   handle unknown node `raw`") — MDX only accepts *user-written* raw
 *   HTML, re-parsed by its own parser;
 * - raw CSS text inside a `<style>` node collides with JSX expression
 *   syntax (`{}`), and Docusaurus ships no passthrough that keeps it.
 *
 * So this package ships the stylesheet as a data-URI
 * `<link rel="stylesheet">` — an ordinary element MDX happily renders —
 * and does it in two halves:
 *
 * ```js
 * // docusaurus.config.mjs
 * import {
 *   remarkChromaglyphDocusaurus,
 *   rehypeChromaglyphDocusaurus,
 * } from '@chromaglyph/docusaurus'
 *
 * export default {
 *   markdown: {
 *     remarkPlugins: [[remarkChromaglyphDocusaurus, { font: './JBM.ttf' }]],
 *     rehypePlugins: [rehypeChromaglyphDocusaurus],
 *   },
 * }
 * ```
 *
 * Turn Docusaurus's own Prism highlighting off so it does not wrap code
 * in colored token spans that fight the font's palette:
 *
 * ```js
 * themeConfig: { prism: { disable: true } }  // see the docs site guide
 * ```
 *
 * All `@chromaglyph/remark` options pass through except `style`.
 */
import type { Root as MdastRoot } from 'mdast'
import type { Root as HastRoot } from 'hast'
import type { Plugin } from 'unified'
import type { VFile } from 'vfile'
import remarkChromaglyph, { type RemarkChromaglyphOptions } from '@chromaglyph/remark'

export type DocusaurusChromaglyphOptions = Omit<RemarkChromaglyphOptions, 'style'>
export type { RemarkChromaglyphOptions } from '@chromaglyph/remark'

/** The stylesheet tag the remark half stripped, handed to the rehype half. */
interface DocusaurusHandoff {
  href?: string
}

declare module 'vfile' {
  interface DataMap {
    chromaglyphDocusaurus?: DocusaurusHandoff
  }
}

// Call-boundary bridging: unified's Plugin type declares `this: Processor`
// (for plugins invoked through processor.use), so calling the plugin and
// its transformer directly needs the plain-function view of the same value
type BasePluginFn = (options: Partial<RemarkChromaglyphOptions>) => (
  tree: MdastRoot,
  file: VFile,
) => Promise<void> | void
const basePlugin = remarkChromaglyph as unknown as BasePluginFn

/**
 * The remark half: runs `@chromaglyph/remark` with `style: 'link'`, then
 * removes the injected `<link>` html node (MDX cannot carry it) and hands
 * its href to the rehype half through `file.data`.
 */
const remarkChromaglyphDocusaurus: Plugin<[DocusaurusChromaglyphOptions?], MdastRoot> = (options) => {
  const inner = basePlugin({ ...options, style: 'link' })
  return async (tree, file: VFile) => {
    await inner(tree, file)
    const head = tree.children[0]
    if (head && head.type === 'html' && 'value' in head) {
      const href = /^<link rel="stylesheet" href="([^"]+)">$/.exec(String(head.value).trim())?.[1]
      if (href) {
        file.data.chromaglyphDocusaurus = { href }
        tree.children.shift()
      }
    }
  }
}

/**
 * The rehype half: re-injects the stylesheet as a real hast `<link>`
 * element — the shape MDX compiles to `_jsx(_components.link, {...})`,
 * which renders an actual `<link>` in the DOM.
 */
const rehypeChromaglyphDocusaurus: Plugin<[], HastRoot> = () => (tree, file: VFile) => {
  const href = file.data.chromaglyphDocusaurus?.href
  if (!href) return
  tree.children.unshift({
    type: 'element',
    tagName: 'link',
    properties: { rel: ['stylesheet'], href },
    children: [],
  })
}

export { remarkChromaglyphDocusaurus, rehypeChromaglyphDocusaurus }
export default remarkChromaglyphDocusaurus
