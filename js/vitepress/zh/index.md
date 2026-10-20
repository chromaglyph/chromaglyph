---
layout: home

hero:
  name: Chromaglyph
  text: 把语法高亮编译进字体
  tagline: DOM 里只有纯文本，颜色住在字形里。没有 span，复制即干净。
  actions:
    - theme: brand
      text: 工作原理
      link: /zh/guide/how-it-works
    - theme: alt
      text: 支持的语言
      link: /zh/guide/languages
    - theme: alt
      text: 接入 VitePress
      link: /zh/guide/integration

features:
  - icon: 🎨
    title: 零 span
    details: 输入一个 TTF、一套主题和你的代码块，输出一个彩色 OpenType 字体。HTML 里只剩纯文本块。
  - icon: 🌗
    title: 一个字体，所有主题
    details: 每套主题是同一字体里的一个调色板。深浅切换只是一次 CSS font-palette 翻转，零重排——试试右上角的开关。
  - icon: 📋
    title: 复制即干净
    details: 复制本站任意代码块，得到的是代码，不是标记。
  - icon: 🛡️
    title: 永不弄坏构建
    details: 无法安全着色的位置保持原色并写入报告；任何失败都回退到普通渲染。
---

本站就是一个真实部署。对下面的代码块查看源码：只有一个 `<pre>` 和纯文本。

```rust
/// A slot = one deduplicated per-theme color vector.
struct Slot {
    colors: Vec<Color>, // one color per theme; equal to the default fg = uncolored
}

fn compile(blocks: &[Block], themes: &[Theme]) -> Font {
    let slots = dedup_slots(blocks, themes);
    // One chained context rule per colored run (GSUB ChainContext format 2);
    // the context is the smallest window that uniquely anchors the occurrence.
    build_font(&slots).expect("never fails - conflicts degrade to uncolored")
}
```
