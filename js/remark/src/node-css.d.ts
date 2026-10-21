// Consumer-side types for @chromaglyph/node/css — the shared CSS/asset
// builder lives in the native-line package (crates/chromaglyph-node) as a
// hand-written module without shipped declarations. Keep the shapes in
// sync with crates/chromaglyph-node/css.mjs.
declare module '@chromaglyph/node/css' {
  import type { BuildResult } from '@chromaglyph/node'

  export interface CssAssetsOptions {
    /** Inline the font as a data: URI (default true); false emits files. */
    embed?: boolean
    /** Directory for emitted font files when embed is false. */
    outDir?: string
    /** Base directory for relative outDir paths (default: process.cwd()). */
    cwd?: string
    /** Scope every rule to `[data-chromaglyph-lang="<lang>"]` (per-language
     *  grouping mode) instead of all `[data-chromaglyph]` elements. */
    lang?: string
  }

  export interface CssAssets {
    css: string
    fontUrl: string
    family: string
    hash8: string
  }

  export function buildCssAssets(result: BuildResult, options?: CssAssetsOptions): CssAssets
}
