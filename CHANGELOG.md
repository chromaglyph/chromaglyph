# Changelog

## Unreleased

## 0.1.3 (2026-10-17)

- **generic mode v1.2 — more languages** (`--generic js | ts | python`
  alongside `rust`): the language spec model is generalized from one
    quote character to multiple string kinds (JS `"`, `'` and `` ` ``
    are independent states; `${...}` degrades to string body for now),
    from one line/block comment to arbitrary delimited spans (Python
    `"""`/`'''` triple-quoted strings beat the simple quote
    longest-first and never close each other), and from one keyword
    scope to keyword groups (`keyword.control` vs `storage.type` vs
    `constant.language`, colored like a real grammar).
    Language-specific identifier characters (`$` in JS) are respected —
    `$return` is a name, not a keyword. The oracle runs curated + 400
    deterministic-random samples per language (generated from the spec
    itself) plus adversarial suites: quote interleaving, unterminated
    states, keyword glue, barrier emoji.
- **string escapes** (all languages): `\` plus any one glyph substitute
  to string body as a two-glyph pair rule, so the shaper walks past an
  escaped quote that has already left the quote class — `\"` no longer
  closes the string early, `\\` does not swallow the closer, and a
  dangling `\` at the line end dies with the state. Verified
  token-by-token with pixel statistics in a real browser (escaped-quote
  content stays string-colored; the following line highlights normally,
  proving correct closure). Backslash has no meaning inside comments and
  triple-quoted spans, which close textually (a `\"""` edge there is a
  documented limitation).
- **model correction, browser-verified**: browsers shape each source
  line of a `pre` as its own run — the `\n` is consumed as the forced
  line break, so contextual rules cannot cross it even when the font
  maps a newline glyph. The v1.1 injected-newline-glyph mechanism
  therefore never worked in a real browser (rustybuzz shapes the whole
  text as a single run and happily verified the fiction). Generic mode
  now models `\n`/`\r` as hard barriers: every state dies at the line
  end (multi-line block comments, template literals and docstrings
  color their opening line only; the continuation line starts fresh and
  plain), and keywords at column 0 or the very end of a line stay
  plain — no in-run glyph to anchor on or look ahead to; indent by one
  space and they color. The injection machinery and `spans_newlines`
  were removed; the generic oracle now judges `mapped()` by the
  produced font's cmap. Verified token-by-token with pixel statistics
  on a rendered page (js/demo `generic.html`): 25/25 positive and
  negative cases align — e.g. `/* one-line block */` fully green, a
  multi-line comment's opening line green with its continuation plain,
  `let` after `*/ ` colored, `def`/`use` at column 0 plain, `$use`
  plain.
- **`@chromaglyph/node`**: `buildFont({ generic: 'rust' | 'js' | 'ts' |
  'python' })` exposes generic mode to JS pipelines (the language is part
  of the content hash; `semantic` is ignored in generic mode), with
  tests running in the workspace (`pnpm test`).
- **ci**: `cli-release.yml` attaches prebuilt `chromaglyph` binaries to
  GitHub Releases on version tags — seven targets (macOS arm64/x64,
  Linux x64/arm64 gnu+musl, Windows x64) built with the same
  zig/cargo-zigbuild pair the napi workflow verified, symbol-stripped,
  ad-hoc signed on macOS (Apple Silicon kills unsigned executables),
  with a `SHA256SUMS` manifest. `workflow_dispatch` runs the same matrix
  as a dry run, keeping the archives as workflow artifacts.
- the CLI crate is renamed **`chromaglyph-cli` → `chromaglyph`** on
  crates.io (`cargo install chromaglyph`); the old name receives one
  final pointer release. The library stays `chromaglyph-core`.

## 0.1.2 (2026-10-16)

- **fix**: the platform packages published from CI templates were missing
  their `main` field, so `require('@chromaglyph/node-<platform>')` failed
  with "Cannot find module" even though the native binary shipped fine —
  only the manually published 0.1.0 darwin-arm64 ever worked. Templates
  now declare `main`, and the publish step enforces it defensively.
  `@chromaglyph/node` and the seven platform binaries re-released;
  `@chromaglyph/remark` / `@chromaglyph/markdown-it` are unaffected and
  stay at 0.1.1.

## 0.1.1 (2026-10-16)

- **generic mode v1.1** (`--generic rust`): build a font that highlights ANY
  text of a language — content no longer needs to be known at build time.
  A state machine is synthesized from a language spec (keywords, `//`
  line comments, double-quoted strings): propagation rules with
  state-class backtracks advance the color glyph by glyph, opening quotes
  enter the string state and closing quotes substitute into a distinct
  end state that stops propagation. ~100 generated rules replace the
  reference project's 706 hand-written ones. The oracle checks the font
  against a reference lexer over curated + 300 deterministic-random
  samples, modeling the shaper faithfully: states die at unmapped chars
  (browser font-fallback run breaks), keywords need mapped punctuation
  glyphs on both sides (buffer-edge and quote/slash-adjacent keywords
  stay plain). v1.1 adds: an injected zero-width newline glyph so shaping
  runs survive line ends (line-start keywords color; the cmap is
  resynthesized from the source mappings), a number state (`3.14` via a
  lookahead rule, `1.max` does not swallow `max`), and block comments
  `/* */` spanning lines. Rust, one language per font, no escapes/nested
  strings; multi-theme still one rule set + one CPAL palette per theme.
- **variation selectors / cmap14**: rule compilation now works in a shaping
  view that matches the shaper. Variation selectors (and other zero-width
  format controls) are transparent: they no longer split colored runs,
  occupy no rule-window position, and stop producing false "no glyph"
  warnings. Unmapped chars (newlines, emoji) are hard barriers — rule
  contexts never cross them, fixing rules that verified green against the
  rustybuzz oracle but could never fire in a browser (font fallback moves
  such chars out of this font's shaping run). Source fonts with a cmap
  format 14 (UVS) subtable pass through untouched, and `base + VS`
  sequences resolve to their variant glyph. The verify oracle now aligns
  shaped glyphs by cluster (rustybuzz emits an extra glyph for unresolved
  VS while browsers delete it — cluster alignment is correct for both).
- **perf**: the global false-match audit is now backed by a suffix-array
  index over the flattened glyph sequence (O(m log n) per query, candidate
  positions cached per occurrence) instead of rescanning every cell on
  every window-growth step (O(n²)). Output is byte-identical to the
  previous implementation on all tests and stress inputs; a 126k-char
  input builds in 0.54s vs 2.18s before.
- **tree-sitter semantic layer** (`--semantic` CLI flag, `semantic: true`
  in buildFont): an optional `chromaglyph-treesitter` crate (the only one
  with C dependencies) parses code with tree-sitter and overlays semantic
  colors — function definitions/calls/methods, parameters, types,
  constants, enum members, struct fields — on top of the syntactic
  highlighting, giving the "editor with a language server" look. Core
  stays pure Rust: it exposes a `SemanticLayer` callback hook
  (`BuildInput.semantic`); scopes a theme does not color are skipped.
  Rust only for now.
- **ci**: release publishing is idempotent (already-published versions
  skip) and authenticates via npm **trusted publishing** (GitHub Actions
  OIDC) — no registry token in any secret; the root `@chromaglyph/node`
  and the pure-JS plugins publish from CI alongside the platform
  binaries.

## 0.1.0 (2026-10-16)

First functional release.

- **exact mode**: build-time tokenization (syntect grammars) + theme
  resolution (VS Code JSON / TextMate themes, include chains, JSONC) compile
  known code blocks into contextual GSUB rules with minimal distinguishing
  contexts and a global false-match audit
- **slot model / multi-theme**: one font, one CPAL palette per theme; theme
  switching is a CSS `@font-palette-values` flip (dark/light auto-switching)
- **color glyphs**: empty copies referenced by COLRv0 layers; 64-glyph
  shaper budget with segment chaining; ambiguous occurrences degrade
  silently — builds never fail
- **verify**: rustybuzz oracle shapes the produced font and diffs every
  glyph, then every palette is read back and checked (runs in `build`)
- **outputs**: WOFF1 (zlib) + ttf, companion CSS, JSON report
- **CLI**: `chromaglyph build` / `verify` (crates.io: `chromaglyph-cli`)
- **JS**: `@chromaglyph/node` napi bindings (batching, sha256 content-hash
  cache in-memory + on-disk, silent degradation), `@chromaglyph/remark` and
  `@chromaglyph/markdown-it` plugins (zero-config data-URI embedding,
  `[data-chromaglyph]` scoping, langs filter)

Known limitations: static TrueType fonts only (CFF/variable rejected),
WOFF1 only (no WOFF2 encoder in pure Rust yet), site-wide batching is
per-document.
