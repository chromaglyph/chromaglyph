# chromaglyph-cli

`chromaglyph` — build and verify fonts with compiled-in syntax highlighting.

```sh
chromaglyph build --font JetBrainsMono-Regular.ttf --input code.rs \
    --theme dark-plus --theme light-plus \
    --output out.woff --css snippet.css --report report.json
chromaglyph verify --font out.woff --input code.rs
```

Build runs the rustybuzz oracle by default (`--no-verify` to skip). Themes:
builtin `dark-plus` / `light-plus` or any VS Code `.json` / TextMate
`.tmTheme` path. Output `.woff` (WOFF1, default) or `.ttf`.

Library: `chromaglyph-core`. JS bindings: `@chromaglyph/node`.
Source: <https://github.com/chromaglyph/chromaglyph>.
