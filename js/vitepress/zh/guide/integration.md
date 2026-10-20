# 接入 VitePress

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

## 为什么要 `style: 'link'`

Vue 框架会把 markdown 输出**当作 Vue 模板编译**，插件默认的内联 `<style>` 活不过这一关：

- **dev**：客户端编译器把 `<style>` 当副作用标签拒绝——页面 404；
- **build（SSG）**：SSR 把 CSS 做了实体转义（`'` → `&#39;`），浏览器不会解码 `<style>` 里的实体，`@font-face` 失效。

`<link rel="stylesheet" href="data:text/css;base64,...">` 是普通元素，属性在两条路径下都能保留。插件给每个 `<pre>` 加的 `v-pre` 同理——没有它 Vue 会把代码里的 `{{ }}` 当插值。

## 外观切换

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

## 保证

- **静默降级**：字体构建失败时文档按无高亮渲染（没有样式表、没有字体），站点构建永不中断。
- **缓存**：内容哈希 = sha256(字体 + 主题源 + 代码块文本 + 选项 + 版本)，进程内 LRU 加可选的磁盘 `cacheDir`（增量构建 49 ms → 5 ms）。
- **作用域**：`font-family` 只作用于 `[data-chromaglyph]`，正文不受影响。
