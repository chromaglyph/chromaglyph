# Chromaglyph

[English](README.md) | 简体中文

**把语法高亮编译进字体。** 给定一个等宽字体、若干主题和内容已知（exact 模式）的
代码块，产出一个彩色 OpenType 字体：代码文本放进 DOM 后**字节不变**，高亮完全
发生在字体内部的 glyph 替换里——不用 `<span>`，复制粘贴零污染。多主题 = 一个
字体多调色板，明暗切换只是一次 CSS `@font-palette-values` 翻转，零重排。

**文档与在线演示：<https://chromaglyph.github.io/chromaglyph/>** —— 文档站本身就是
一次 chromaglyph 部署：站内每个代码块都是纯文本，颜色由字体完成。从
[快速上手](https://chromaglyph.github.io/chromaglyph/zh/guide/getting-started)开始。

灵感来自 [Cobble / FontWithASyntaxHighlighter](https://github.com/hlotvonen/FontWithASyntaxHighlighter)：
原版在字体里手写了一个「有限状态机词法分析器」；Chromaglyph 反其道而行——
**内容在构建期已知，解析在构建期完成**，字体里只留下查表。

## 工作原理

```
tokenize+theme → compile → fontbuild → verify
```

- **tokenize + theme**：[syntect](https://github.com/trishume/syntect) 用真实
  TextMate 语法解析，再在**每个主题**下把每个字符解析成具体颜色。内置 shiki
  [tm-themes](https://github.com/shikijs/textmate-grammars-themes) 主题集全部
  64 个主题（`chromaglyph themes` 可列出），也支持你自己的 VS Code `.json` /
  `.tmTheme` 文件。
- **compile**：每个着色 run 生成一条链式上下文替换规则（GSUB ChainContext），
  上下文取**恰好能唯一锚定该位置的最小窗口**，并在全局范围内做假匹配审计。
  无法唯一区分的位置降级为不染色并记入 warnings——**构建永不失败**。规则只依赖
  颜色 slot，因此**一套 GSUB 服务所有主题**，切换调色板零成本。
- **fontbuild**：彩色副本是**零轮廓字形** + COLRv0 层引用基础字形（每 slot
  体积成本 ≈ 0），CPAL 一主题一调色板。exact 模式只保留代码块实际用到的字形
  （真实页面 ~8 倍缩小）。输入支持静态 TTF、name-keyed CFF（OTF）、WOFF1/WOFF2
  与 `.ttc`；输出 WOFF1/WOFF2/sfnt。
- **verify**：harfrust 进程内整形产出的字体，逐字符比对替换后的 glyph id；
  再按调色板逐一读回 CPAL 核对颜色。`build` 默认自跑。

两种模式：

- **exact**（`--input`）：内容构建期已知；多语言共用一个字体（全局消歧）。
  `--ligatures` 保留源字体的连字——`!=`、`->` 及上下文变体都按字体本来的
  样子渲染，同时保持高亮。
- **generic**（`--generic rust|js|ts|python|go|java|c|cpp|csharp|php|ruby|lua|shell|html|css`）：
  不需要指定内容——字体高亮该语言的**任意**文本，纯靠字形替换推进状态。
  一语言一字体，着色为行内语义。
- 可选 `--semantic`（tree-sitter；Rust / JS / TS / Python / Java）：把语义
  span——函数、参数、类型、成员——覆盖在语法色之上，像开了语言服务器的编辑器。

细节、权衡与在线示例见文档站：[工作原理](https://chromaglyph.github.io/chromaglyph/zh/guide/how-it-works)、
[支持的语言](https://chromaglyph.github.io/chromaglyph/zh/guide/languages)、
[主题](https://chromaglyph.github.io/chromaglyph/zh/guide/themes)。

## 安装

- **预编译二进制**：GitHub Releases 提供 7 个目标，附带 `SHA256SUMS`
  （macOS arm64/x64、Linux gnu/musl、Windows x64；musl 全静态）。
- **cargo**：`cargo install chromaglyph`。
- **npm**：`@chromaglyph/node`，以及 `@chromaglyph/remark`、
  `@chromaglyph/markdown-it`、`@chromaglyph/astro`、`@chromaglyph/docusaurus`
  与 `@chromaglyph/eleventy` 插件。

## 快速上手

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf \
    --input hello.rs \                    # --lang 按扩展名自动探测
    --theme dark-plus --theme light-plus \
    --output out.woff --css snippet.css   # + --report report.json 查看警告

chromaglyph verify --font out.woff --input hello.rs

# 通用模式：不指定内容，字体高亮该语言的任意文本
chromaglyph build --font JetBrainsMono-Regular.ttf --generic rust \
    --output generic.woff --css generic.css
```

生成的 CSS 把主题转为 `@font-palette-values`，默认跟随 `prefers-color-scheme`；
class 切换明暗（VitePress 的 `html.dark` 等）只需覆盖两个变量。输出格式：
`.woff`（WOFF1，默认）、`.woff2`（再小 15–30%）、`.ttf`（裸 sfnt）。

从源码构建：`cargo build --release`，然后 `cargo run -p chromaglyph -- build …`。

把字体接入页面见[快速上手](https://chromaglyph.github.io/chromaglyph/zh/guide/getting-started)，
VitePress / Astro / Docusaurus / Eleventy 接入见
[接入指南](https://chromaglyph.github.io/chromaglyph/zh/guide/integration)。

## JS 插件

`@chromaglyph/remark` 与 `@chromaglyph/markdown-it` 把**一个文档的所有
fenced 代码块批处理合并进一个字体**（跨语言全局消歧），字体默认以 data: URI
内联，CSS 作用域限定 `[data-chromaglyph]`（prose 永不受影响），任何失败静默
降级。内容哈希缓存把增量站点构建从 49ms 降到 5ms；`grouping: 'lang'` 可选
一语言一字体。框架适配器构建其上：`@chromaglyph/astro`（Astro / Starlight）、
`@chromaglyph/docusaurus`（MDX 管线）、`@chromaglyph/eleventy`（markdown-it）。
`@chromaglyph/node` 暴露 `buildFont` / `buildFontSafe` 及缓存 API。各包细节见
其自身 README，完整历史与各版本已知限制见 [CHANGELOG.md](CHANGELOG.md)。

## 已知限制

- **源字体必须覆盖代码文本的字符**：无字形的字符（如 JetBrains Mono 之于
  CJK）经浏览器回退渲染、**无法着色**；中文注释/字符串请用带 CJK 的等宽
  字体（如 Sarasa Mono SC）。构建 warning 会列出具体缺失字符。
- 可变 TrueType 源钉到默认实例；CFF2（可变 OTF）与 CID-keyed CFF 源会带
  明确报错地拒绝。
- generic 模式为行内语义（浏览器对 `pre` 逐行整形，状态在换行处死亡），
  且一语言一字体。

## 技术选型

`read-fonts` / `skrifa` / `write-fonts`（fontations 系）· harfrust（整形
预言机）· syntect（语法/主题）· 可选 tree-sitter（语义层）· flate2（WOFF1）
· ttf2woff2（WOFF2 编码）· wuff（WOFF2 解码）。

## License

Apache-2.0。`themes/*.json`（VS Code theme-defaults，MIT）与测试 fixture
`JetBrainsMono-Regular.ttf`（OFL）遵循其自身上游许可。
