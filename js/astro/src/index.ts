/**
 * @chromaglyph/astro — chromaglyph syntax highlighting for Astro &
 * Starlight.
 *
 * ## Starlight (zero config wiring)
 *
 * ```js
 * // astro.config.mjs
 * import starlight from '@astrojs/starlight'
 * import { starlightPlugin } from '@chromaglyph/astro'
 *
 * export default defineConfig({
 *   integrations: [starlight({
 *     plugins: [starlightPlugin({
 *       font: './JetBrainsMono-Regular.ttf',
 *       cacheDir: '.chromaglyph-cache',
 *     })],
 *   })],
 * })
 * ```
 *
 * The plugin disables Starlight's Shiki highlighting (its inline-colored
 * `<span>`s are exactly what chromaglyph eliminates, and they fight the
 * font's palette) and installs the remark pipeline with
 * `style: 'link'` — a `<link>` is an inert ordinary element in any
 * downstream pipeline.
 *
 * ## Plain Astro
 *
 * Use `@chromaglyph/remark` directly — Astro's markdown pipeline keeps
 * raw html, so the default inline `<style>` works:
 *
 * ```js
 * // astro.config.mjs
 * import remarkChromaglyph from '@chromaglyph/remark'
 * export default defineConfig({
 *   markdown: {
 *     syntaxHighlight: false, // turn Shiki off, the font does the coloring
 *     remarkPlugins: [[remarkChromaglyph, { font: './JBM.ttf' }]],
 *   },
 * })
 * ```
 */
import remarkChromaglyph, { type RemarkChromaglyphOptions } from '@chromaglyph/remark'

export type StarlightChromaglyphOptions = Omit<RemarkChromaglyphOptions, 'style'>
export type { RemarkChromaglyphOptions } from '@chromaglyph/remark'

/**
 * Minimal structural view of Starlight's plugin API
 * (`@astrojs/starlight/types`). Declared structurally — importing the real
 * types drags in Astro *virtual modules* (`virtual:starlight/
 * plugin-translations`) that only resolve inside an actual Starlight
 * build, which would break consumers' typechecks.
 */
export interface StarlightPluginAPI {
  config: Record<string, unknown>
  updateConfig: (config: Record<string, unknown>) => void
  astroConfig?: {
    markdown?: {
      remarkPlugins?: unknown[]
      [key: string]: unknown
    }
    [key: string]: unknown
  }
}

/** Minimal structural view of `StarlightPlugin` (see {@link StarlightPluginAPI}). */
export interface StarlightPlugin {
  name: string
  hooks: {
    setup: (api: StarlightPluginAPI) => void | Promise<void>
  }
}

/**
 * Starlight plugin: turns off Shiki and installs chromaglyph's remark
 * pipeline with `style: 'link'` forced. Existing user `remarkPlugins` are
 * preserved (appended after).
 */
export function starlightPlugin(options: StarlightChromaglyphOptions): StarlightPlugin {
  return {
    name: 'chromaglyph',
    hooks: {
      setup({ updateConfig, astroConfig }) {
        // Starlight deep-merges updateConfig payloads, but arrays are
        // replaced — spread whatever the user already configured so this
        // never clobbers other remark plugins
        const markdown = astroConfig?.markdown ?? {}
        updateConfig({
          markdown: {
            ...markdown,
            syntaxHighlight: false,
            remarkPlugins: [
              ...(markdown.remarkPlugins ?? []),
              [remarkChromaglyph, { ...options, style: 'link' as const }],
            ],
          },
        })
      },
    },
  }
}

export default starlightPlugin
