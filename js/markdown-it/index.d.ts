import type MarkdownIt from 'markdown-it'
import type { Buffer } from 'node:buffer'

export interface MarkdownItChromaglyphOptions {
  /** Path or buffer of the static TrueType source font (required). */
  font: string | Uint8Array
  /** Builtin theme names or paths to VS Code .json / .tmTheme files. */
  themes?: string[]
  /** Restrict processing to these languages; others keep default styling. */
  langs?: string[] | null
  /** Inline the font as a data: URI in the injected <style> (default true). */
  embed?: boolean
  /** Directory for emitted font files when `embed` is false. */
  outDir?: string
  /** Run the rustybuzz oracle (default true). */
  verify?: boolean
  /** On-disk cache directory for incremental rebuilds. */
  cacheDir?: string
  /** Base directory for relative `font`/`outDir` paths (default: cwd). */
  cwd?: string
}

declare const markdownItChromaglyph: MarkdownIt.PluginSimpleWithOptions<
  MarkdownItChromaglyphOptions | undefined
>

export default markdownItChromaglyph
export { markdownItChromaglyph }
