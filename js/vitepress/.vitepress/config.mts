import { resolve } from 'node:path'
import { defineConfig } from 'vitepress'
import chromaglyph from '@chromaglyph/markdown-it'

// The vitepress CLI runs with cwd = this package dir; the font is the
// in-repo fixture (JetBrainsMono, static TTF)
const font = resolve(
  process.cwd(),
  '../../crates/chromaglyph-core/tests/fixtures/JetBrainsMono-wght-VF.ttf',
)

export default defineConfig({
  title: 'Chromaglyph',
  // GitHub Pages serves the project site under /chromaglyph/; local dev and
  // any root-hosted deployment leave this unset
  base: process.env.VITEPRESS_BASE ?? '/',
  appearance: true,
  cleanUrls: true,
  markdown: {
    config(md) {
      // VitePress's built-in Shiki wraps tokens in inline-colored <span>s —
      // exactly what chromaglyph eliminates (and double-highlighting fights
      // the font's palette). Hand the fence back to markdown-it's plain
      // escaping rule; coloring is done entirely by the font's COLR/CPAL.
      md.options.highlight = null
      md.use(chromaglyph, {
        font,
        themes: ['dark-plus', 'light-plus'],
        // Vue compiles markdown output as a template: a <style> tag is
        // rejected by the client compiler / entity-escaped by SSR, only a
        // <link rel="stylesheet"> reaches the browser (see plugin README)
        style: 'link',
        // keep the font's own ligatures (!= -> === …) while highlighted;
        // mixed-color sites degrade to per-char text (colors win)
        semantic: true,
        ligatures: true,
        cacheDir: '.chromaglyph-cache',
      })
    },
  },
  locales: {
    root: {
      label: 'English',
      lang: 'en-US',
      description:
        'Syntax highlighting compiled into a font: the DOM stays plain text, colors live in the glyphs, copy & paste stays clean.',
      themeConfig: {
        nav: [{ text: 'Guide', link: '/guide/how-it-works', activeMatch: '/guide/' }],
        sidebar: [
          {
            text: 'Guide',
            items: [
              { text: 'How it works', link: '/guide/how-it-works' },
              { text: 'Languages', link: '/guide/languages' },
              { text: 'Themes', link: '/guide/themes' },
              { text: 'VitePress integration', link: '/guide/integration' },
            ],
          },
        ],
      },
    },
    zh: {
      label: '简体中文',
      lang: 'zh-CN',
      link: '/zh/',
      description:
        '把语法高亮编译进字体：DOM 里只有纯文本，颜色住在字形里，复制即干净。',
      themeConfig: {
        nav: [{ text: '指南', link: '/zh/guide/how-it-works', activeMatch: '/zh/guide/' }],
        sidebar: [
          {
            text: '指南',
            items: [
              { text: '工作原理', link: '/zh/guide/how-it-works' },
              { text: '支持的语言', link: '/zh/guide/languages' },
              { text: '主题', link: '/zh/guide/themes' },
              { text: '接入 VitePress', link: '/zh/guide/integration' },
            ],
          },
        ],
        outline: { label: '本页目录' },
        docFooter: { prev: '上一页', next: '下一页' },
        darkModeSwitchLabel: '外观',
        lightModeSwitchTitle: '切换到浅色模式',
        darkModeSwitchTitle: '切换到深色模式',
        sidebarMenuLabel: '菜单',
        returnToTopLabel: '回到顶部',
        langMenuLabel: '切换语言',
      },
    },
  },
  themeConfig: {
    logo: '/logo.svg',
    socialLinks: [
      { icon: 'github', link: 'https://github.com/chromaglyph/chromaglyph' },
    ],
  },
})
