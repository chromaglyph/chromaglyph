# 接入指南

chromaglyph 为大家实际用来发文档的框架提供了适配包。每个适配包存在的理由都是该框架的管线有一条通用插件猜不到的硬约束——适配器把这些知识吸收掉,你不必操心:

| 框架 | 包 | 渲染管线 | 样式表形态 |
|------|-----|---------|-----------|
| VitePress | `@chromaglyph/markdown-it` | markdown-it | `style: 'link'` |
| Astro / Starlight | `@chromaglyph/astro` | remark | `<link>`(强制) |
| Docusaurus | `@chromaglyph/docusaurus` | remark + MDX | `<link>`(强制) |
| Eleventy | `@chromaglyph/eleventy` | markdown-it | 内联 `<style>` |

引擎选项(`font`、`themes`、`langs`、`grouping`、`semantic`、`ligatures`、`format`、`embed`、`cacheDir`)在所有框架下含义一致——完整选项表见各包 README。

## VitePress

VitePress 通过 markdown-it 渲染，所以 `@chromaglyph/markdown-it` 只需两个框架开关加一个 `md.use`：

```ts
// .vitepress/config.mts
import { resolve } from 'node:path'
import { defineConfig } from 'vitepress'
import chromaglyph from '@chromaglyph/markdown-it'

export default defineConfig({
  markdown: {
    config(md) {
      // 1) Disable Shiki: it wraps tokens in colored <span>s, which is
      //    exactly what chromaglyph eliminates; hand the fence back to
      //    markdown-it's plain escaping rule
      ;(md.options as { highlight: unknown }).highlight = null
      // 2) style: 'link' - see below
      md.use(chromaglyph, {
        font: resolve(process.cwd(), 'JetBrainsMono-Regular.ttf'),
        themes: ['dark-plus', 'light-plus'],
        style: 'link',
        // keep the font's own ligatures (!= -> === …) while highlighted;
        // mixed-color sites degrade to per-char text (colors win)
        ligatures: true,
        cacheDir: '.chromaglyph-cache',
      })
    },
  },
})
```

### 为什么要 `style: 'link'`

Vue 框架会把 markdown 输出**当作 Vue 模板编译**，插件默认的内联 `<style>` 活不过这一关：

- **dev**：客户端编译器把 `<style>` 当副作用标签拒绝——页面 404；
- **build（SSG）**：SSR 把 CSS 做了实体转义（`'` → `&#39;`），浏览器不会解码 `<style>` 里的实体，`@font-face` 失效。

`<link rel="stylesheet" href="data:text/css;base64,...">` 是普通元素，属性在两条路径下都能保留。插件给每个 `<pre>` 加的 `v-pre` 同理——没有它 Vue 会把代码里的 `{{ }}` 当插值。

### 外观切换

生成的 CSS 默认用 `prefers-color-scheme` 做深浅切换，背景取主题的编辑器背景。两者都在 CSS 变量之后，基于 class 的切换只需在一处覆盖，无需运行时 JS：

```css
/* .vitepress/theme/style.css */
:root {
  --chromaglyph-palette: var(--chromaglyph-light-palette);
  --chromaglyph-bg: var(--vp-code-block-bg); /* VitePress 自己的代码块背景 */
}
html.dark {
  --chromaglyph-palette: var(--chromaglyph-dark-palette);
}
```

- 每页的样式表定义了无哈希别名 `--chromaglyph-dark-palette` / `--chromaglyph-light-palette`，并以 `font-palette: var(--chromaglyph-palette, …)` 读取。一旦你设置了 `--chromaglyph-palette`，它同时压过深色默认值和媒体查询；由 VitePress 的 `html.dark` 驱动切换——依然零重排。
- `--chromaglyph-bg` 让代码块脱离主题烘焙的 `#1e1e1e` / `#ffffff`；指向 `var(--vp-code-block-bg)` 后，`<pre>` 和它的 `<code>` 一起跟随 VitePress 自己的背景系统。

这两个变量适用于任何基于 class 的切换（Nuxt 的 `dark-mode`、你自己翻转的 `.dark`）。跟随系统配色的站点什么都不用做。

## Astro / Starlight

Starlight 一条插件入口全搞定——自动关掉 Shiki 并装好 remark 管线：

```js
// astro.config.mjs
import starlight from '@astrojs/starlight'
import { starlightPlugin } from '@chromaglyph/astro'
import { defineConfig } from 'astro/config'

export default defineConfig({
  integrations: [
    starlight({
      plugins: [starlightPlugin({
        font: './JetBrainsMono-Regular.ttf',
        cacheDir: '.chromaglyph-cache',
      })],
    }),
  ],
})
```

不带 Starlight 的纯 Astro 直接用 `@chromaglyph/remark`，配 `markdown.syntaxHighlight: false`——Astro 的管线保留原始 html，默认的内联 `<style>` 在那里可用。

## Docusaurus

Docusaurus 通过 **MDX** 渲染 markdown，产物会编译成 JSX——全套件里约束最狠的管线。两种显而易见的样式表形态都会挂:注入的 `html` 节点死在 MDX 的 hast→estree 步骤("Cannot handle unknown node `raw`"),`<style>` 里的 CSS 文本又与 JSX 表达式语法(`{}`)相撞。适配器把工作拆到两个插件列表,样式表以 data-URI `<link>` 落地:

```js
// docusaurus.config.mjs
import {
  remarkChromaglyphDocusaurus,
  rehypeChromaglyphDocusaurus,
} from '@chromaglyph/docusaurus'

export default {
  markdown: {
    remarkPlugins: [[remarkChromaglyphDocusaurus, { font: './JBM.ttf' }]],
    rehypePlugins: [rehypeChromaglyphDocusaurus],
  },
  themeConfig: {
    prism: { disable: true }, // Prism 的着色 span 会跟字体调色板打架
  },
}
```

## Eleventy

Eleventy 通过 markdown-it 渲染;`@chromaglyph/eleventy` 提供现成的 `amendLibrary` 回调。注意用 ESM 配置(chromaglyph 全家桶与 unified 生态一样仅 ESM):

```js
// eleventy.config.mjs
import { withChromaglyph } from '@chromaglyph/eleventy'

export default function (eleventyConfig) {
  eleventyConfig.amendLibrary('md', withChromaglyph({
    font: './JetBrainsMono-Regular.ttf',
    cacheDir: '.chromaglyph-cache',
  }))
}
```

Eleventy 的产物是纯 HTML,默认的内联 `<style>` 正是正确形态。

## 保证

所有适配器共享引擎的保证:

- **静默降级**：字体构建失败时文档按无高亮渲染（没有样式表、没有字体），站点构建永不中断。
- **缓存**：内容哈希 = sha256(字体 + 主题源 + 代码块文本 + 选项 + 版本)，进程内 LRU 加可选的磁盘 `cacheDir`（增量构建 49 ms → 5 ms）。
- **作用域**：`font-family` 只作用于 `[data-chromaglyph]`，正文不受影响。
