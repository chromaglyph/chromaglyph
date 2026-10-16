# Changelog

## Unreleased

- **perf**: the global false-match audit is now backed by a suffix-array
  index over the flattened glyph sequence (O(m log n) per query, candidate
  positions cached per occurrence) instead of rescanning every cell on
  every window-growth step (O(n²)). Output is byte-identical to the
  previous implementation on all tests and stress inputs; a 126k-char
  input builds in 0.54s vs 2.18s before.
- **ci**: release publishing is idempotent (already-published platform
  versions skip), npm auth is wired via `setup-node` `registry-url`, and
  the root `@chromaglyph/node` package publishes from CI alongside the
  platform binaries. Requires the `NPM_TOKEN` repository secret.

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
