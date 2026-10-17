---
layout: home

hero:
  name: Chromaglyph
  text: Syntax highlighting, compiled into a font
  tagline: The code in the DOM stays byte-identical — all highlighting happens inside glyph substitutions. No span soup, clean copy & paste.
  actions:
    - theme: brand
      text: How it works
      link: /guide/how-it-works
    - theme: alt
      text: Eight languages
      link: /guide/languages
    - theme: alt
      text: VitePress integration
      link: /guide/integration

features:
  - icon: 🎨
    title: Zero-span highlighting
    details: Given a static TTF, themes and known code blocks, produce a color OpenType font. Rules depend only on slots, so one GSUB serves every theme.
  - icon: 🌗
    title: One font, many palettes
    details: Multiple themes = one font with multiple CPAL palettes. Switching appearance is a single CSS font-palette flip with zero reflow (this site's toggle is wired up).
  - icon: 📋
    title: Copy & paste stays clean
    details: Try copying the block below — or any block on this site. What you paste is plain code text, not a single extra tag or style.
  - icon: 🛡️
    title: Builds never fail
    details: Occurrences that cannot be uniquely anchored degrade to uncolored and land in report.warnings; every JS-side failure silently falls back to normal rendering.
---

This site is itself a real chromaglyph deployment: the highlighting of the Rust block below does not live in the DOM — view source and you will find only a `` <pre> `` and plain text.

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
