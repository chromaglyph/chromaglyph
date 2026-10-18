import { resolve } from 'node:path'
import { defineConfig } from 'vitepress'
import chromaglyph from '@chromaglyph/markdown-it'

// The vitepress CLI runs with cwd = this package dir; the font is the
// in-repo fixture (JetBrainsMono, static TTF)
const font = resolve(
  process.cwd(),
  '../../crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf',
)

export default defineConfig({
  title: 'Chromaglyph',
  description:
    'Syntax highlighting compiled into a font: the DOM stays byte-identical, highlighting happens inside glyph substitutions, copy & paste stays clean.',
  appearance: true,
  cleanUrls: true,
  markdown: {
    config(md) {
      // VitePress's built-in Shiki wraps tokens in inline-colored <span>s —
      // exactly what chromaglyph eliminates (and double-highlighting fights
      // the font's palette). Hand the fence back to markdown-it's plain
      // escaping rule; coloring is done entirely by the font's COLR/CPAL.
      ;(md.options as { highlight: unknown }).highlight = null
      md.use(chromaglyph, {
        font,
        themes: ['dark-plus', 'light-plus'],
        // Vue compiles markdown output as a template: a <style> tag is
        // rejected by the client compiler / entity-escaped by SSR, only a
        // <link rel="stylesheet"> reaches the browser (see plugin README)
        style: 'link',
        // keep the font's own ligatures (!= -> === …) while highlighted;
        // mixed-color sites degrade to per-char text (colors win)
        ligatures: true,
        semantic: true,
        cacheDir: '.chromaglyph-cache',
      })
    },
  },
  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/how-it-works', activeMatch: '/guide/' },
    ],
    sidebar: [
      {
        text: 'Guide',
        items: [
          { text: 'How it works', link: '/guide/how-it-works' },
          { text: 'Six languages', link: '/guide/languages' },
          { text: 'VitePress integration', link: '/guide/integration' },
        ],
      },
    ],
    socialLinks: [
      { icon: 'github', link: 'https://github.com/chromaglyph/chromaglyph' },
    ],
  },
})
