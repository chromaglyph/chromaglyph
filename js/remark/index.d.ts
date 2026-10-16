import type { Plugin } from 'unified'
import type { VFile } from 'vfile'

export interface RemarkChromaglyphOptions {
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
}

export interface ChromaglyphFileInfo {
  cached: boolean
  contentHash: string
  css: string
  fontUrl: string
  embed: boolean
  outDir: string | null
  warnings: string[]
  themes: { name: string; background?: string }[]
}

declare const remarkChromaglyph: Plugin<
  [RemarkChromaglyphOptions?],
  never,
  never
>

export default remarkChromaglyph
export { remarkChromaglyph }

declare module 'vfile' {
  interface DataMap {
    chromaglyph?: ChromaglyphFileInfo
  }
}
