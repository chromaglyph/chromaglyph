# Changelog

## Unreleased

- **plugins: `grouping: 'page' | 'lang'`** (`@chromaglyph/remark`,
  `@chromaglyph/markdown-it`, default `'page'`): opt into one font per
  language instead of one per document. Fences carry
  `data-chromaglyph-lang` and the generated CSS scopes each font (and its
  palette aliases) to its own language's blocks, so several fonts coexist
  on one page while a site-level `--chromaglyph-palette` /
  `--chromaglyph-bg` still overrides every scope. Editing one language's
  snippet rebuilds only that language's font — the others hit the cache —
  and identical text under different languages can now highlight each
  its own way (impossible in one font, where the same glyph sequence
  always substitutes identically). Per-language failures degrade
  per-language. Post-subsetting the cost is ~10K of base font per extra
  language on a page; `md.chromaglyph()` returns one `BuildResult` per
  language and remark's `file.data.chromaglyph` gains `{ grouping:
  'lang', fonts: [...] }` in this mode.

- **glyph subsetting (exact mode)**: the produced font now ships only the
  glyphs the blocks' text resolves to — plus `.notdef` and the transitive
  components of composite glyphs — renumbered and rewritten consistently
  through cells, classes, alt copies, hmtx, COLR and the generated GSUB.
  The cmap is rebuilt from the survivors and re-emits the used cmap14
  variation sequences as a format-14 subtable, so `base + VS` text keeps
  resolving to its variant glyph (e2e-tested). Real pages drop ~8x: the
  demo's 7-language page goes from ~106K to ~15K woff (base64-embedded
  into HTML: ~142K → ~20K), a typical 3-language page from ~105K to
  ~13K. Generic mode keeps the full charset (its font must highlight any
  text); source fonts carrying other glyph-referencing tables (embedded
  bitmaps, SVG, an existing COLR, MATH) skip subsetting and ship whole,
  exactly as before. `Report.total_glyphs` is now the produced font's
  count (subset + colored copies); `source_glyphs` keeps describing the
  source font. Note: the content hash covers inputs, not engine bytes —
  caches built from a source snapshot between 0.2.1 and this change share
  hashes with subset builds; delete such a `.chromaglyph-cache` once.

- **generic mode: C** (`--generic c`, node `buildFont({generic: 'c'})`):
  `"` strings and `'` char literals with escapes, both comment kinds,
  control-flow / storage-type / storage-modifier keyword groups,
  `sizeof` (blue via `keyword.other.c` — `keyword.operator.word`
  resolves to the default foreground in dark-plus and would not color),
  `true`/`false`/`NULL` constants and numbers. Preprocessor directive
  words (`include`, `define`, `ifdef`, `endif`, `pragma`, …) color
  purple like an editor's directives — the `#` glyph is not part of the
  identifier to the font and stays plain; the words color wherever they
  appear as identifiers, so plausible-identifier directives (`error`,
  `warning`, `line`) were left out on purpose. Curated + spec-derived
  oracle samples and an adversarial suite (quote interleaving,
  unterminated states, directive-word soup, line-start keywords);
  326 rules on the fixture font.

- **css: var-driven palette + background — class-toggle dark mode with
  zero JS**. The companion CSS (CLI `--css` and both JS plugins) now
  reads `font-palette: var(--chromaglyph-palette, …)` and
  `background: var(--chromaglyph-bg, …)`, and defines hash-free aliases
  `--chromaglyph-dark-palette` / `--chromaglyph-light-palette` in the
  same stylesheet as the font (per-page, so SPA page swaps stay correct).
  A site that toggles a class instead of following the OS scheme —
  VitePress's `html.dark`, Nuxt color mode — takes over BOTH the dark
  default and the `prefers-color-scheme` branch in one place:

  ```css
  :root     { --chromaglyph-palette: var(--chromaglyph-light-palette);
              --chromaglyph-bg: var(--vp-code-block-bg); }
  html.dark { --chromaglyph-palette: var(--chromaglyph-dark-palette); }
  ```

  This replaces the ~40-line runtime shim the VitePress demo shipped in
  0.2.1 (it scanned `document.styleSheets` for the hash-derived palette
  names — different per page — plus a MutationObserver for SPA
  navigation, with backgrounds hardcoded). `--chromaglyph-bg` also
  detaches from the theme-baked `#1e1e1e`/`#ffffff`: pointing it at the
  framework's own code-block variable makes the block follow the
  framework's theming (radius, dark variants) on the `<pre>` and its
  `<code>` child together. Plain sites that define neither var render
  exactly as before. Note: the on-disk cache stores the CSS verbatim —
  caches built by ≤0.2.1 serve the old shape until the version bump
  rotates the content hash (as usual for the node package).

- **semantic layer: Java** (`--semantic` with `java` blocks /
  `buildFont({semantic: true})`): method/constructor declarations and
  invocation names, class/interface/enum/record names and `new` types,
  parameters (typed, `String...` spread, single-param lambdas), field
  declarations vs `static final` literal constants vs lambda/method-ref
  bindings, enum constants, field accesses (`obj.x`, `this.x`), and
  variable reads/writes — locals, loop bindings and call receivers get
  the semantic variable blue. Import/package paths and annotations stay
  in their syntactic colors (no symbol table; a receiver like `System`
  renders as a variable read). New `tree-sitter-java` dependency in the
  optional crate — core stays pure Rust.
- **semantic layer: Python variables** (promised in 0.2.1): bare
  identifiers read/write as `variable.other.readwrite` — locals, loop
  bindings, assignment targets and call receivers (`obj.run()`: `obj` is
  a read). Keyword-argument labels, import paths and decorator names
  keep their syntactic colors; parameters were extended to
  `typed_default_parameter` and `*args`/`**kw` splats. Base classes in a
  class header now read as types.
- **fix: Python UPPER_CASE constants rendered as variables** — the
  CONSTANT span for `MAX = 100` was pushed and then overwritten by a
  VARIABLE span for the same binding site (later spans win in apply
  order); the old test asserted `contains` and never caught it. The
  assignment target is now excluded from the variable branch and a
  regression test asserts exactly one span covers the binding site.
- **fix: Rust method receivers colored as variables** — `v.iter()` used
  to leave `v` uncolored (every identifier under a field access was
  excluded, including the receiver); only the field position is excluded
  now, so the receiver reads as a variable like in an editor.
- docs: the CLI `--semantic` help still said "currently rust" (stale
  since 0.2.0) — now lists rust, js/ts, python, java.

## 0.2.1 (2026-10-17)

- **plugins: opt-in `semantic` option** (`@chromaglyph/remark`,
  `@chromaglyph/markdown-it`): pass-through to `buildFont({semantic:
  true})`, default off — same-page cross-language consistency and
  ecosystem-baseline (syntactic) rendering stay the default; one line
  opts a site into the language-server look. Tests cover the option
  joining the content hash.
- **fix: `semantic` was missing from the node content hash** — an
  enabled build could wrongly hit the plain font's disk cache. Note for
  upgraders: engine changes since 0.2.0 (TypeScript grammar, variable
  spans) do not invalidate caches built against the 0.2.0 binary until
  the next version bumps `CACHE_VERSION`; a stale `.chromaglyph-cache`
  built before these fixes should be deleted once.
- **semantic layer: Rust variable reads** color as VS Code's semantic
  blue (`variable.other.readwrite` → #9CDCFE) via tree-sitter — bare
  identifiers, excluding every position another branch scopes (call and
  macro names, fields, paths, declaration names, parameters). JS/TS
  already get variable colors from their grammar; Python/Java variable
  spans are future work. The VitePress demo site ships with semantic on.

- **fix: exact mode could not highlight TypeScript** — syntect's default
  syntax set does not contain a TypeScript grammar, so `ts`/`tsx`
  fences silently resolved to plain text: zero coloring, zero warnings.
  The tokenizer now uses the two-face superset (TypeScript, TSX and
  other missing grammars; `syntect-fancy` backend, still pure Rust, no
  onig) cached once per process. Verified end-to-end on the VitePress
  site: interface/readonly/string blue, calls in dark-plus' function
  yellow. Binaries grow with the larger syntax dump.

- **build warnings name the uncovered characters**: when a token contains
  chars with no glyph in the font (CJK comments in a Latin-only font,
  emoji, …), the warning now lists the actual missing characters and,
  for CJK ranges, appends a hint that they render via browser fallback
  and cannot be highlighted — use a CJK-capable monospace font (e.g.
  Sarasa Mono SC). Previously the warning said "typically trailing
  newlines", which hid the real cause. Behavior is unchanged (the
  oracle-verified barrier model); an e2e test locks it.

- **markdown-it: Vue-framework stylesheet injection** — new `style:
  'inline' | 'link'` option. Vue-template frameworks (VitePress, Nuxt
  Content) compile the markdown output as a Vue component template, where
  the previously injected `<style>` tag never reaches the browser: the
  client compiler rejects side-effect tags (dev serves a 404 for the page)
  and SSR entity-escapes the CSS text (killing `@font-face`). `style:
  'link'` injects `<link rel="stylesheet">` instead — an ordinary element
  that survives both paths (data: URI stylesheet with `embed: true`, a
  `.css` file next to the woff with `embed: false`). Fences are now also
  tagged `v-pre` so `{{ }}` inside code is never treated as Vue
  interpolation. Plain markdown-it pipelines keep the default `'inline'`.
  Found by running a real VitePress site (`js/vitepress`, new): also
  documents the two framework switches — `md.options.highlight = null`
  (Shiki's colored spans would double-highlight against the font palette)
  and a ~40-line theme shim syncing `font-palette` with VitePress's
  `html.dark` toggle (pixel-verified in both directions).

## 0.2.0 (2026-10-17)

- **semantic layer: JS, TS and Python** (`--semantic` /
  `buildFont({semantic: true})`): the tree-sitter overlay now covers
  function/method definitions and calls, parameters (including TS
  typed/optional wrappers), properties vs call-target methods, class
  and `new` types, TS built-in types (`predefined_type` in the grammar)
  and JS `const` function bindings / literal constants / Python
  UPPER_CASE assignment constants. Grammar quirks handled: python's
  call node is `call` (not `call_expression`). With this, M4 closes —
  generic mode covers six languages with escapes and JS template
  interpolation; multi-language-per-font is decided against
  (cross-language false positives).

- **template interpolation** (JS/TS): `` `hi `${`name`}` bye` `` renders
  like an editor — string segments orange, `${`/`}` keep the string
  color, the expression renders uncolored, and the string resumes after
  the closer. One flat level: the first `}` resumes, nested openers are
  expression body (no stack in GSUB, by design). The opener is a
  two-glyph rule from the string state; expression glyphs propagate an
  interpolation state whose slot paints the default foreground. `$`,
  `{` and `}` get dedicated classes in interpolating languages (compact
  `){return` style loses its keyword anchor — accepted). Python
  f-strings deliberately not covered: the `f` prefix is a plain
  identifier to the font, indistinguishable from `r"..."` raw strings
  and `"{} {}".format()`, which would regress. Oracle samples gain
  hole/empty/nested pieces; browser pixel check: 5/5 align.

- **generic mode: Go and Java** (`--generic go | java`, node
  `buildFont({generic})` alike): Go gets `"` strings, backtick raw
  strings with a new per-kind `escapes: false` switch — raw strings have
  no escapes in Go, so a backslash before the closing backtick does not
  suppress it — and `'` rune literals; Java gets `"` strings, `'` char
  literals and declaration/modifier keyword groups (`class`/`char`
  render in dark-plus' teal `storage.type.java`, exactly like VS Code).
  Curated + 400 spec-derived random samples per language in the oracle;
  browser pixel check on the demo page.

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
