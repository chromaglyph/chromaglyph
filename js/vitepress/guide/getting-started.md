# Getting Started

You need two things: a monospace font file, and code whose content is known
at build time. Everything else — themes, subsetting, ligature handling,
verification — the build takes care of.

## Install

- **Prebuilt binaries** from
  [GitHub Releases](https://github.com/chromaglyph/chromaglyph/releases): 7
  targets with `SHA256SUMS` (macOS arm64/x64, Linux gnu/musl, Windows x64;
  musl is fully static). macOS builds are ad-hoc signed — first run may
  need `xattr -d com.apple.quarantine <binary>`.
- **npm CLI**: `npm install -g chromaglyph` — the same CLI, a prebuilt
  binary per platform.
- **cargo**: `cargo install chromaglyph`
- **Node API**: `@chromaglyph/node` wraps the same engine for Node.

## Your first font

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf \
    --input hello.rs \                    # --lang auto-detects by extension
    --theme dark-plus --theme light-plus \
    --output out.woff --css chromaglyph.css \
    --report report.json
```

Three files come out:

- `out.woff` — the color font: every theme is a palette inside it
  (`dark-plus` = palette 0, `light-plus` = palette 1).
- `chromaglyph.css` — an `@font-face` plus one `@font-palette-values` rule
  per theme, ready to `<link>` into any page.
- `report.json` — per-block stats and warnings (occurrences that can't be
  uniquely anchored degrade to uncolored instead of failing the build).

Repeat `--input` for as many blocks as you like — languages can be mixed,
disambiguation is global, and the font is subset to just the glyphs your
code uses. `chromaglyph verify --font out.woff --input hello.rs` re-checks
a font independently; `build` already runs the same check.

Any theme name from [Themes](./themes) works (`chromaglyph themes` prints
the list), and your own VS Code `.json` / `.tmTheme` files work as paths.

## Put it on a page

```html
<link rel="stylesheet" href="chromaglyph.css" />
<pre>fn main() {
    println!("hello, chromaglyph");
}</pre>
```

That's the whole integration: the `<pre>` stays **plain text** — no spans,
no classes. View source on any block on this site and you'll see the same
shape; the colors you're reading now are glyphs in a font.

The generated CSS follows `prefers-color-scheme` by default. Sites that
toggle a class instead (VitePress `html.dark`, Nuxt `dark-mode`, your own
`.dark`) override two variables in one place:

```css
:root     { --chromaglyph-palette: var(--chromaglyph-light-palette);
            --chromaglyph-bg: <your code-block background>; }
html.dark { --chromaglyph-palette: var(--chromaglyph-dark-palette); }
```

Zero reflow either way — a palette flip never changes metrics.

## No content at build time? Generic mode

Static site with hand-written snippets? Use exact mode. But if the code is
edited after the font is built (a playground, a live editor), build a
generic font instead — it highlights **any** text of one language by
advancing state through glyph substitution:

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf --generic rust \
    --output generic.woff --css generic.css
```

One language per font, and coloring is line-scoped: browsers shape `<pre>`
line by line, so multi-line constructs (block comments, template strings)
color their starting line only. See [Languages](./languages) for what each
mode covers.

## Docs sites

For VitePress, Astro/Starlight, Docusaurus and Eleventy there are adapters
that batch every fenced block of a document into one font at build time,
cache by content hash, and silently fall back to unhighlighted output on
any failure — see [Integrations](./integration).

## Next steps

- [How it works](./how-it-works) — the slot model, GSUB rules and the
  verify oracle, with live examples.
- [Languages](./languages) — what each mode covers, with live blocks
  rendered by the actual font.
- [Themes](./themes) — all 64 builtin themes with palette previews.
- [Integrations](./integration) — one package per docs framework.
