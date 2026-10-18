# Chromaglyph

[English](README.md) | 简体中文

**把语法高亮编译进字体。** 给定一个等宽字体、若干主题和内容已知（exact 模式）的
代码块，产出一个彩色 OpenType 字体：代码文本放进 DOM 后**字节不变**，高亮完全
发生在字体内部的 glyph 替换里——不用 `<span>`，复制粘贴零污染。多主题 = 一个
字体多调色板，明暗切换只是一次 CSS `@font-palette-values` 翻转，零重排。

灵感来自 [Cobble / FontWithASyntaxHighlighter](https://github.com/hlotvonen/FontWithASyntaxHighlighter)：
原版在字体里手写了一个「有限状态机词法分析器」（706 条 calt 规则逐字符推进
状态）；Chromaglyph 反其道而行——**内容在构建期已知，解析在构建期完成**，字体
里只留下查表。

## 工作原理

```
tokenize+theme → compile → fontbuild → verify
```

- **tokenize + theme**：[syntect](https://github.com/trishume/syntect)
  （bat 的引擎，纯 Rust fancy-regex，零 C 依赖）用真实 TextMate 语法解析，再在
  **每个主题**下把每个字符解析成具体颜色。主题支持 VS Code `.json`（含 JSONC、
  `include` 链合并）与 `.tmTheme`；内置 `dark-plus` / `light-plus`。
- **compile**：每个着色 run 生成一条链式上下文替换规则（GSUB
  ChainContext），上下文取**恰好能唯一锚定该出现位置的最小窗口**，并在全部
  块的全局范围内做假匹配审计。无法唯一区分的位置降级为不染色并记入
  report.warnings——**构建永不失败**。规则只依赖颜色 slot，因此**一套 GSUB
  服务所有主题**，切换调色板零成本。
- **fontbuild**：源字体表尽量按字节原样保留；彩色副本是**零轮廓字形** +
  COLRv0 层引用基础字形（每 slot 体积成本 ≈ 0），CPAL 一主题一调色板。替换掉
  源字体自带的 GSUB/GPOS/GDEF 和 AAT 布局表（避免干扰）。字体源：静态
  TrueType（TTF）与 name-keyed CFF（OTF，字节级 CharStrings 手术把空的
  彩色字形以 `endchar` charstring 重建字形集——exact 模式对 CFF 同样
  子集化（被丢弃字形的 charstring 与名字串一并去掉，hinting 子程序
  原样保留）；WOFF1/WOFF2 容器与 `.ttc` 集合（`--font-index`）输入时
  自动解包；可变 TrueType 钉到默认实例（glyf 轮廓本身就是默认实例，
  直接丢弃变体表即可）。CFF2（可变 OTF 轮廓）与 CID-keyed CFF 会带
  明确报错地拒绝。
- **verify**（预言机）：rustybuzz 进程内整形产出的字体，逐字符比对替换后的
  glyph id（整形与主题无关，验一次即可）；再按调色板逐一读回 CPAL 核对颜色。
  `build` 默认自跑。

两种模式：

- **exact**（`--input`）：内容构建期已知。多语言共用一个字体（全局消歧），
  0.2.2 起字体**只保留代码块实际用到的字形**（真实页面 ~8 倍缩小）。
  0.2.3 起 `--ligatures` 保留源字体的连字：构建期用源字体对每个代码块做
  一次 shaping，把每个替换重物化为规则——`!=`、`->` 以及 JetBrains Mono
  的上下文变体都按字体本来的样子渲染，同时保持高亮。跨颜色的连字位点
  退化为逐字符渲染（颜色优先——一个字形只能有一种颜色）。
- **generic**（`--generic rust|js|ts|python|go|java|c|cpp|csharp|php|ruby|lua|shell|html|css`）：不需要指定内容——
  一个字体高亮该语言的**任意**文本（关键字、注释、字符串、数字），纯靠字形
  替换推进状态，思路即原项目的 FSM，但从语言 spec 自动合成，只需约 100 条
  规则。一语言一字体。着色为行内语义：浏览器对 `pre` 逐行整形，状态在换行处
  死亡（跨行的块注释/模板字符串只着色起始行）。
- 可选 `--semantic`（tree-sitter；Rust / JS / TS / Python / Java）：把语义
  span——函数、参数、类型、成员——覆盖在语法色之上，像开了语言服务器的编辑器。

## 安装

- **预编译二进制**：GitHub Releases 提供 7 个目标（macOS arm64/x64、
  Linux x64/arm64 的 gnu/musl、Windows x64），每个版本附带 `SHA256SUMS`；
  musl 构建为全静态。macOS 包为 ad-hoc 签名，浏览器下载后首次运行可能需要
  `xattr -d com.apple.quarantine <binary>`。
- **cargo**：`cargo install chromaglyph`。
- **npm**：`@chromaglyph/node`，以及 `@chromaglyph/remark`、
  `@chromaglyph/markdown-it` 插件。

## 用法

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf \
    --input examples/hello.rs \             # --lang auto 按扩展名探测
    --theme dark-plus --theme light-plus \  # 或 my-theme.json / my.tmTheme，可重复
    --ligatures \                           # 保留字体的 != -> 连字，且带高亮
    --output out.woff --css snippet.css --report report.json

chromaglyph verify --font out.woff --input examples/hello.rs

# 通用模式：不指定内容，字体高亮该语言的任意文本
chromaglyph build --font JetBrainsMono-Regular.ttf --generic rust \
    --output generic.woff --css generic.css
```

生成的 CSS（节选）——主题转为 `@font-palette-values`，默认跟随
`prefers-color-scheme` 明暗自动切换：

```css
@font-face { font-family: 'chromaglyph'; src: url('out.woff') format('woff'); }
@font-palette-values --chromaglyph-dark-plus  { font-family: chromaglyph; base-palette: 0; }
@font-palette-values --chromaglyph-light-plus { font-family: chromaglyph; base-palette: 1; }
:root { /* 免哈希别名；class 切换明暗见下 */
  --chromaglyph-dark-palette: --chromaglyph-dark-plus;
  --chromaglyph-light-palette: --chromaglyph-light-plus;
}
pre, code {
  font-family: chromaglyph, monospace;
  font-palette: var(--chromaglyph-palette, var(--chromaglyph-dark-palette));
  background: var(--chromaglyph-bg, #1e1e1e);
}
@media (prefers-color-scheme: light) {
  pre, code { font-palette: var(--chromaglyph-palette, var(--chromaglyph-light-palette));
              background: var(--chromaglyph-bg, #ffffff); }
}
/* class 切换明暗的站点（VitePress 的 html.dark 等）只需在主题 CSS 里接管：
   :root     { --chromaglyph-palette: var(--chromaglyph-light-palette);
               --chromaglyph-bg: <你的代码块背景变量>; }
   html.dark { --chromaglyph-palette: var(--chromaglyph-dark-palette); } */
```

输出扩展名选 `.ttf` 则写裸 sfnt，`.woff`（默认）写 WOFF1（zlib），`.woff2`
写 WOFF2（Brotli，再小 15–30%——单流压缩让全部表共享一个窗口）。两种轮廓
风格都能装进 WOFF2：TrueType 原样、CFF 走空变换（规范没有定义 CFF 的表变
换）。glyf/loca 表变换保持关闭（省下几个百分点的体积，换来绕开 Chromium
解码器的已知坑）。

从源码构建：

```sh
cargo build --release
cargo run -p chromaglyph -- build \
    --font crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf \
    --input examples/hello.rs \
    --output target/demo/chromaglyph-demo.woff --css target/demo/chromaglyph-demo.css
python3 -m http.server -d target/demo 8143   # 打开 index.html 看明暗双调色板
```

## JS 插件

`@chromaglyph/remark` 与 `@chromaglyph/markdown-it`（VitePress 通道）把
**一个文档的所有 fenced 代码块批处理合并进一个字体**（跨语言全局消歧），字体
默认以 data: URI 内联，CSS 作用域限定 `[data-chromaglyph]`（prose 永不受
影响），任何失败静默降级。内容哈希缓存（进程内 LRU + 可选磁盘）把增量站点
构建从 49ms 降到 5ms；0.2.2 起可选 `grouping: 'lang'`，一语言一字体。

```js
import { unified } from 'unified'
import remarkParse from 'remark-parse'
import remarkRehype from 'remark-rehype'
import rehypeStringify from 'rehype-stringify'
import remarkChromaglyph from '@chromaglyph/remark'

const file = await unified()
  .use(remarkParse)
  .use(remarkChromaglyph, { font: './JetBrainsMono-Regular.ttf' })
  .use(remarkRehype, { allowDangerousHtml: true })
  .use(rehypeStringify, { allowDangerousHtml: true })
  .process(markdown)
```

`@chromaglyph/node` 暴露 `buildFont` / `buildFontSafe`（失败返回 `null`）及
缓存 API。各包细节见其自身 README，完整历史与各版本已知限制见
[CHANGELOG.md](CHANGELOG.md)。

## 已知限制

- **源字体必须覆盖代码文本的字符**：无字形的字符（如 JetBrains Mono 之于
  CJK）经浏览器回退渲染、**无法着色**（着色发生在本字形的替换里）——周围的
  ASCII 部分照常着色。构建 warning 会列出具体缺失字符；中文注释/字符串请用
  带 CJK 的等宽字体（如 Sarasa Mono SC）。
- 输入支持静态 TrueType（TTF）、name-keyed CFF（OTF）、WOFF1、WOFF2
  与 `.ttc` 集合（`--font-index` 选择成员）；可变 TrueType 钉到默认实例。
  CFF2（可变 OTF）与 CID-keyed CFF 拒绝。输出：WOFF1（默认）、WOFF2
  （`.woff2`，两种轮廓风格均可）或裸 sfnt（`.ttf`）。
- generic 模式为行内语义（见上）、一语言一字体——合并语言会让 TS 的 `type`
  给 Rust 块里的 `type` 上色。
- 通用模式的关键词/数字需要行内有前置边界字形，行首关键字保持无色
  （字体内置词法器的既有限制，其后内容正常着色）。

## 尺寸对比（examples/hello.rs，114 字符，双主题）

| 表 | 本项目 | 参考实现 FontWithASyntaxHighlighter |
|----|-------|-------|
| GSUB | 1.2 KB（exact 模式，仅锚定规则） | 39 KB（FSM 全状态机） |
| COLR | 0.4 KB（空轮廓 + 层引用） | 12.7 KB（轮廓复制） |
| CPAL | 40 B（2 调色板 × 6 slot） | 50 B（1 调色板） |
| 规则数 | 9 | 706 |

自我压力测试（`compile.rs` 本身，31535 字符，双主题）：622 条规则、7 slot、
241 彩色副本，构建 0.14s。全局假匹配审计基于字形序列的后缀数组索引（O(n log n)
查询），126k 字符输入下构建 0.54s，输出与逐位置扫描版字节级一致。

## 技术选型

`read-fonts` / `skrifa` / `write-fonts`（fontations 系）· rustybuzz（整形
预言机）· syntect（语法/主题）· 可选 tree-sitter（语义层）· flate2（WOFF1）
· ttf2woff2（WOFF2 编码）· wuff（WOFF2 解码）。

## License

Apache-2.0。`themes/*.json`（VS Code theme-defaults，MIT）与测试 fixture
`JetBrainsMono-Regular.ttf`（OFL）遵循其自身上游许可。
