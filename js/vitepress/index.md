---
layout: home

hero:
  name: Chromaglyph
  text: Syntax highlighting, compiled into a font
  tagline: The DOM holds plain text; the colors live in the glyphs. No spans, clean copy & paste.
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: How it works
      link: /guide/how-it-works
    - theme: alt
      text: Languages
      link: /guide/languages

features:
  - icon: 🎨
    title: Zero spans
    details: A TTF, a theme and your code blocks go in; one color OpenType font comes out. The HTML stays a plain text block.
  - icon: 🌗
    title: One font, every theme
    details: Each theme is a palette inside the same font. Dark/light is a single CSS font-palette flip with zero reflow — try the toggle.
  - icon: 📋
    title: Clean copy & paste
    details: Copy any block on this site. You get code, not markup.
  - icon: 🛡️
    title: Never breaks a build
    details: Spots that cannot be colored safely stay plain and are reported; any failure falls back to normal rendering.
---

This site is a real deployment. View source on the block below: one `<pre>`, plain text.

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
