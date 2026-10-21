import { test } from 'vitest'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { unified } from 'unified'
import remarkParse from 'remark-parse'
import remarkRehype from 'remark-rehype'
import rehypeStringify from 'rehype-stringify'
import { starlightPlugin } from '../src/index'

const FONT = readFileSync('../../crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf')
const MARKDOWN = `# demo

\`\`\`rust
fn main() {
    let msg = "hello chromaglyph";
}
\`\`\`
`

/** Run the plugin's setup hook against a mock StarlightPluginAPI. */
function setupCapture(astroConfig?: { markdown?: { remarkPlugins?: unknown[] } }) {
  const updates: { markdown: { syntaxHighlight: unknown; remarkPlugins: unknown[] } }[] = []
  const plugin = starlightPlugin({ font: FONT })
  plugin.hooks.setup({
    updateConfig: (config: unknown) => updates.push(config as never),
    astroConfig,
  } as never)
  return updates
}

test('the Starlight plugin disables Shiki and installs the remark pipeline', () => {
  const updates = setupCapture()
  assert.equal(updates.length, 1, 'one updateConfig call')
  const update = updates[0]!
  assert.equal(update.markdown.syntaxHighlight, false, 'Shiki highlighting off')
  assert.equal(update.markdown.remarkPlugins.length, 1, 'chromaglyph remark plugin installed')
  const [plugin, options] = update.markdown.remarkPlugins[0] as [unknown, { style?: string }]
  assert.equal(typeof plugin, 'function')
  assert.equal(options.style, 'link', 'style forced to link for Starlight')
})

test('existing user remark plugins are preserved, chromaglyph appended', () => {
  const userPlugin = () => {}
  const updates = setupCapture({ markdown: { remarkPlugins: [userPlugin] } })
  const update = updates[0]!
  assert.equal(update.markdown.remarkPlugins[0], userPlugin, 'user plugin kept')
  assert.equal(update.markdown.remarkPlugins.length, 2, 'chromaglyph appended')
})

test('the installed remark entry works through an Astro-shaped unified pipeline', async () => {
  const updates = setupCapture()
  const update = updates[0]!
  const [plugin, options] = update.markdown.remarkPlugins[0] as [unknown, Record<string, unknown>]
  // exactly the markdown chain Astro runs: parse → chromaglyph →
  // rehype (raw html kept) → stringify
  const html = String(
    await unified()
      .use(remarkParse)
      // @ts-expect-error plugin tuple comes from the captured config
      .use(plugin, options)
      .use(remarkRehype, { allowDangerousHtml: true })
      .use(rehypeStringify, { allowDangerousHtml: true })
      .process(MARKDOWN),
  )
  assert.ok(/<link rel="stylesheet" href="data:text\/css;base64,/.test(html), 'link stylesheet injected')
  assert.ok(/data-chromaglyph="0"/.test(html), 'code block marked')
  assert.ok(html.includes('fn main() {'), 'code text intact')
})
