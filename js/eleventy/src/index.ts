/**
 * @chromaglyph/eleventy — chromaglyph syntax highlighting for Eleventy.
 *
 * Eleventy renders markdown through markdown-it, so highlighting drops in
 * via `eleventyConfig.amendLibrary('md', …)`. This package provides that
 * callback preconfigured for a static-HTML pipeline:
 *
 * ```js
 * // eleventy.config.mjs (Eleventy 3; use an ESM config — the chromaglyph
 * // packages are ESM-only, like the markdown-it plugin they wrap)
 * import { withChromaglyph } from '@chromaglyph/eleventy'
 *
 * export default function (eleventyConfig) {
 *   eleventyConfig.amendLibrary('md', withChromaglyph({
 *     font: './JetBrainsMono-Regular.ttf',
 *     cacheDir: '.chromaglyph-cache',
 *   }))
 * }
 * ```
 *
 * The injected stylesheet is an inline `<style>` (Eleventy output is plain
 * HTML; nothing downstream compiles it as a component template). All
 * `@chromaglyph/markdown-it` options pass through unchanged.
 */
import type MarkdownIt from 'markdown-it'
import chromaglyph, { type MarkdownItChromaglyphOptions } from '@chromaglyph/markdown-it'

export type { MarkdownItChromaglyphOptions } from '@chromaglyph/markdown-it'

/**
 * Build the `amendLibrary('md', …)` callback. Relative `font`/`outDir`
 * paths resolve against the process cwd (where `eleventy` runs).
 */
export function withChromaglyph(options: MarkdownItChromaglyphOptions): (md: MarkdownIt) => void {
  return (md) => {
    md.use(chromaglyph, options)
  }
}

export default withChromaglyph
