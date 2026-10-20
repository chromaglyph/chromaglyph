# Themes

Chromaglyph ships the builtin themes of shiki's
[tm-themes](https://github.com/shikijs/textmate-grammars-themes) collection:
the same VS Code theme files VS Code and shiki use, vendored byte-for-byte
and embedded in the binary (≈250 KB gzipped). Any name on this page works
wherever a theme is expected:

- **Plugins** (`@chromaglyph/markdown-it`, `@chromaglyph/remark`):
  `themes: ['github-dark', 'github-light']`. The first theme becomes the
  dark palette and the second the light one — see
  [integration](./integration) for the CSS variables that flip them.
- **CLI**: `--theme github-dark --theme github-light`; `chromaglyph themes`
  prints this list (`--json` for scripts).
- **Node**: `listThemes()` returns the registry with per-scope swatches,
  enough to build a theme picker without compiling a font.

Your own VS Code `.json` or TextMate `.tmTheme` file works in all three
places as a path. `dark` / `light` alias `dark-plus` / `light-plus`.

The previews below are plain HTML painted with each theme's palette — the
colors the real highlighter resolves for keywords, strings, comments,
numbers, functions, types and variables — not the font itself, so you can
compare themes before building one.

<ThemeStats />

## Dark

<ThemeGallery type="dark" />

## Light

<ThemeGallery type="light" />

## Credits

Every builtin theme stays under its authors' license — MIT or Apache-2.0;
themes under other licenses in the collection (aurora-x, GPL-3.0) are not
shipped. The upstream notices are vendored alongside the theme files as
`crates/chromaglyph-core/themes/tm/NOTICE`.

<ThemeCredits />
