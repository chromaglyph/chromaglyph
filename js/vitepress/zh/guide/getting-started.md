# 快速上手

你需要两样东西：一个等宽字体文件，和内容在构建期已知的代码。其余的——
主题、子集化、连字处理、校验——构建都会处理。

## 安装

- **预编译二进制**：[GitHub Releases](https://github.com/chromaglyph/chromaglyph/releases)
  提供 7 个目标，附带 `SHA256SUMS`（macOS arm64/x64、Linux gnu/musl、
  Windows x64；musl 全静态）。macOS 包为 ad-hoc 签名，首次运行可能需要
  `xattr -d com.apple.quarantine <binary>`。
- **npm CLI**：`npm install -g chromaglyph`——同一 CLI，按平台安装
  预编译二进制。
- **cargo**：`cargo install chromaglyph`。
- **Node API**：`@chromaglyph/node` 在 Node 里封装了同一引擎。

## 第一个字体

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf \
    --input hello.rs \                    # --lang 按扩展名自动探测
    --theme dark-plus --theme light-plus \
    --output out.woff --css chromaglyph.css \
    --report report.json
```

产出三个文件：

- `out.woff`——彩色字体：每个主题都是里面的一个调色板
  （`dark-plus` = 调色板 0，`light-plus` = 调色板 1）。
- `chromaglyph.css`——一条 `@font-face` 加上每个主题一条
  `@font-palette-values` 规则，`<link>` 进任意页面即可。
- `report.json`——逐块统计与警告（无法唯一锚定的位置降级为不染色，
  而不是让构建失败）。

`--input` 可以重复任意多次——语言可以混用，全局消歧，字体只保留代码
实际用到的字形。`chromaglyph verify --font out.woff --input hello.rs`
可以独立复检一个字体；`build` 默认已跑过同一套校验。

[主题](./themes)页里的任意主题名都可以用（`chromaglyph themes` 打印
列表），你自己的 VS Code `.json` / `.tmTheme` 文件以路径传入也可以。

## 放到页面上

```html
<link rel="stylesheet" href="chromaglyph.css" />
<pre>fn main() {
    println!("hello, chromaglyph");
}</pre>
```

接入就这么多：`<pre>` 仍然是**纯文本**——没有 span，没有 class。查看
本站任意代码块的源码都是同样的形状；你现在看到的颜色是字体里的字形。

生成的 CSS 默认跟随 `prefers-color-scheme`。用 class 切换明暗的站点
（VitePress 的 `html.dark`、Nuxt 的 `dark-mode`、你自己的 `.dark`）
只需在一处覆盖两个变量：

```css
:root     { --chromaglyph-palette: var(--chromaglyph-light-palette);
            --chromaglyph-bg: <你的代码块背景>; }
html.dark { --chromaglyph-palette: var(--chromaglyph-dark-palette); }
```

两种方式都零重排——调色板翻转不改变任何度量。

## 构建期拿不到内容？generic 模式

静态站点、代码是手写的？用 exact 模式。但如果代码在字体构建之后还会
被编辑（playground、在线编辑器），改用 generic 字体——它纯靠字形替换
推进状态，高亮该语言的**任意**文本：

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf --generic rust \
    --output generic.woff --css generic.css
```

一语言一字体，着色为行内语义：浏览器对 `pre` 逐行整形，跨行结构
（块注释、模板字符串）只着色起始行。两种模式的覆盖范围见
[支持的语言](./languages)。

## 文档站

VitePress、Astro/Starlight、Docusaurus、Eleventy 各有适配器：构建期把
一个文档的全部 fenced 代码块合批进一个字体，按内容哈希缓存，任何失败
静默回退到不高亮——见[接入指南](./integration)。

## 接下来

- [工作原理](./how-it-works)——slot 模型、GSUB 规则与 verify 预言机，
  附在线示例。
- [支持的语言](./languages)——两种模式各覆盖哪些语言，代码块由真实字体渲染。
- [主题](./themes)——全部 64 个内置主题的调色板预览。
- [接入指南](./integration)——每个文档框架一个包。
