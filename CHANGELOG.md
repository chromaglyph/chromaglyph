# Changelog

## Unreleased

- **generic mode: CSS** (`--generic css`) — the second non-keyword
  language, on a new `BlockKind` (property-sheet) construct: selector
  words color after a boundary or `.`/`:`; property names are keywords
  that fire only when a `:` follows (`a:hover` never matches — `hover`
  is not a property and the colon-check is per word, so pseudo-classes
  stay selectors); values ride in an uncolored regime until `;` `{` `}`
  with numbers, strings and comments colored by the ordinary
  machinery, and value-position keywords (`!important`, `inherit`)
  carry the regime through their own glyphs. Multi-line rule blocks
  cannot carry state across lines (browsers shape line by line) —
  continuation property lines work through the property keywords'
  colon lookahead instead. Ships a ~365-name standard property list.
  The oracle-driven bring-up fixed eight engine/lexer divergences,
  three of them structural:
  - keyword letters carry 1:1 classes, so "word-ish" continuation sets
    must be computed per character AND per letter class (selectors,
    tag states and value propagation all missed letters before).
  - chain format-2 subtables carry THREE ClassDef copies; state
    classes covering most of the font (string bodies, value carriers)
    made each ~6 bytes/glyph, so a "36 KiB" rule chunk serialized
    past 64 KiB and the rule-set offsets overflowed. The chunk budget
    now subtracts the classdef overhead, and state-family alts are
    allocated kind-major so classes collapse into contiguous ranges
    (this also un-broke C#/Java, which had crossed the same line).
  - a value regime (and every state family) must die at newline
    barriers in the reference lexer exactly like the engine's
    propagation rules; the block DFA was carrying it across.
  Also new: `KeywordGroup.value` (value-position keyword groups) and
  the anchoring rule that a word/number may open after a closer that
  actually closed on this line (`"a" 3px`, `/* c */ .x`) — never after
  one that died at a newline.

- **generic mode: HTML** (`--generic html` | `htm`) — the first
  non-keyword language: a new `TagKind` spec construct synthesizes a
  markup tag state machine (name → attribute names → `=` → values,
  quoted per quote kind) out of the same glyph-class machinery as
  strings/spans. Tag names, attribute names and attribute values each
  carry their own scope (`entity.name.tag.html`,
  `entity.other.attribute-name.html`, `string.quoted.double.html`);
  the delimiters (`< > / =` and the quotes) ride along as *uncolored
  state carriers* — alt glyphs with no palette entry of their own, the
  interpolation-body trick reused for a whole state family. Text
  between tags stays plain, `<!-- -->` comments and `&...;` entities
  are spans. Approximations, all oracle-verified and documented:
  `<3`/`<>`/a spaced `< b` stay plain (the opener needs an identifier
  lookahead), entities are recognized in text only (tag/value bodies
  win by rule order), a `"` inside `'…'` is body (one quoted-value
  state per quote kind), an unclosed tag dies at the newline
  (line-scoped like everything else), and `<script>`/`<style>`
  embedded languages are not modeled. Two engine fixes found on the
  way: continuation classes are computed per character, not per class
  identity (html's `-` is both a data-attribute char and a `<!--`
  delimiter — the class split made the font and the reference lexer
  disagree); the quote-per-state split above. Browser smoke gains a
  `generic-html` case — OTS-accepted in headless Chromium with
  tag-blue / value-orange / comment-green pixels.

- **generic mode: PHP, Ruby, Lua, Shell** (`--generic php` / `ruby` |
  `rb` / `lua` / `shell` | `sh` | `bash`, node `buildFont({generic:
  ...})`): all four fit the existing spec machinery —
  - **PHP**: `$` is an identifier char (`$return` is a name, `$var`
    inside double quotes stays string body), `{$x}` interpolation
    holes, `//` `#` `/* */` comments.
  - **Ruby**: `#{x}` interpolation, `#` comments; symbols (`:name`)
    and heredocs deliberately not modeled (a symbol opener would
    swallow the line; heredocs span lines by design).
  - **Lua**: `--` and `--[[ ]]` comments, `[[` long strings (a span —
    `--[[` wins longest-first over both `--` and `[[`).
  - **Shell**: `'` raw strings (no escapes — exactly shell semantics),
    `"` strings with `${var}` holes, `#` comments (mid-word hashes
    color too — a glyph FSM cannot see word starts; documented).
  - **engine/lexer fix found by shell's `$1`**: interpolation
    opener/closer chars (`$ { }`) carry dedicated glyph classes in the
    engine and do not anchor keywords or numbers there — the reference
    lexer's boundary predicate disagreed (latent since JS templates;
    `{keyword` would have diverged too). The lexer now matches the
    engine, verified by new interpolation-heavy adversarial samples
    for all four languages.

- **generic mode: C#** (`--generic csharp` / `cs` / `c#`, node
  `buildFont({generic: 'csharp'})`): keyword groups
  (control / storage.type / storage.modifier / other / constants), `"`
  strings and `'` char literals with escapes, `//` + `///` doc + `/* */`
  comments, numbers. C#-specific calls: verbatim `@"..."` strings are a
  span kind — the two-char opener beats the plain `"` string at the same
  position, backslashes are plain body, and the doubled-`""` escape is
  approximated by closing at the first quote (line-scoped best effort);
  verbatim identifiers (`@return`) stay uncolored because `@` lives in
  a span opener and is therefore not a boundary glyph; `$"interpolated"`
  colors as an ordinary string (`$` plain, holes are string body).
  Contextual keywords (`value`, `when`, `where`) are deliberately absent
  — a stateless in-font lexer would over-color ordinary identifiers.
  Curated + adversarial oracle samples and lexer unit tests included.

- **CFF subsetting (exact mode)** — OTF sources now subset like TTF
  sources instead of shipping the full charset: the CharStrings INDEX
  is rebuilt with only the glyphs the blocks use (charstrings are
  self-contained, so there is no composite closure to walk), the
  charset is re-emitted in format 0, and the String INDEX is trimmed to
  the strings still referenced — the shipped glyphs' names plus the Top
  DICT's own strings — with SIDs renumbered from 391 (the glyph-name
  strings of dropped glyphs were 13KB of a 33KB table on Source Code
  Pro). Same code content that built an 85KB woff before now builds
  20KB. Hinting subroutines (global + local) ride along untouched —
  trimming those needs a Type2 interpreter and stays future work;
  they are the remaining size gap to the TTF path. Verified through
  the rustybuzz oracle, read-fonts' own CFF parser (charset coverage +
  per-glyph name identity across the subset) and the browser smoke
  test's OTS path.

- **WOFF2 input** — `wOF2` files decode to sfnt before loading via the
  pure-Rust `wuff` decoder (MIT; brings only `brotli-decompressor`),
  covering the most common shape of "a font downloaded from the web":
  brotli-compressed tables plus the transformed glyf/loca streams.
  Tested against a real Google Fonts file (JetBrains Mono latin
  subset). Output remains WOFF1 — no pure-Rust WOFF2 encoder exists.

## 0.3.0 (2026-10-18)

- **font compatibility: OTF/CFF, variable TrueType, WOFF1 and `.ttc`
  inputs** — the pipeline no longer requires a static TrueType TTF:

  - **CFF/OTF sources** (name-keyed): the colored copies are appended to
    the CFF table as bare `endchar` charstrings via byte-level surgery
    (`cff.rs`): the CharStrings INDEX grows (offsets re-encoded at the
    offSize the grown table needs), the charset is extended to cover the
    new glyphs (SID 0 — a name several glyphs may share; range formats
    split at their nLeft width), and the whole table is re-laid-out with
    the Top DICT's offset operands (charset 15, Encoding 16, CharStrings
    17, Private-offset 18) recomputed in fixed 5-byte form so the dict's
    length is layout-stable between the measuring and patching passes.
    Name/String/GSubr indexes and the Private DICT + local subrs blob
    are copied byte-for-byte. The output stays CFF-flavored (OTTO sfnt;
    FontBuilder picks the flavor from the table set, and the WOFF
    wrapper/unwrap now carry the flavor field through instead of
    hardcoding TrueType). Exact mode does not subset CFF sources (the
    full charset ships, like the other subset-blocking cases); CID-keyed
    CFF (Top DICT `ROS`, FDArray/FDSelect per-glyph font dicts) and CFF2
    (variable-OTF outlines, which need real instancing) are rejected
    with explicit errors. `--ligatures` works against OTF sources — the
    build-time shaper runs on the normalized sfnt either way.
  - **variable TrueType fonts** are pinned to their default instance:
    the glyf outlines already *are* the default, so accepting the font
    is just dropping the variation machinery (`fvar`/`avar`/`cvar`/
    `gvar`/`HVAR`/`VVAR`/`MVAR`/`STAT`) from the output — subsetting and
    `--ligatures` apply exactly as for static TTFs. The report carries a
    "variable font pinned to its default instance" warning.
  - **WOFF1 containers** are unwrapped to sfnt before loading (they
    previously failed at FontRef with a parse error).
  - **`.ttc` collections**: `--font-index N` (CLI) / `fontIndex`
    (`@chromaglyph/node`) extracts member N; out-of-range indexes are a
    load error. Extraction *relocates* the member into a standalone sfnt
    (header + directory re-emitted, table data copied, offsets
    recomputed): table offsets inside a member's directory are absolute
    positions in the TTC file — read-fonts' `CollectionRef`, HarfBuzz and
    Chromium all resolve them against the whole collection — and Apple's
    system collections (Menlo, every macOS `.ttc`) pack the member
    directories at the front while the data they point at is shared at
    the back, so slicing bytes out was never an option. Verified against
    Menlo.ttc members 0 and 3 (different builds selected per index).
    Also fixed along the way: the ligature shaper was handed the raw
    input bytes and would have silently degraded for WOFF/TTC inputs; it
    now shapes the normalized sfnt.
  - `NotTrueType`/`VariableFont` collapse into
    `UnsupportedFont(String)` with a message that says what to do (e.g.
    "build from the static OTF or the TTF instead").
  - Tests: e2e builds from Source Code Pro Regular (OTF, including a
    `--ligatures` pass and a charset cross-check through read-fonts' own
    CFF parser), JetBrains Mono `wght` VF (asserts the variation tables
    are gone and the pinning warning is present), a wrapped-WOFF input
    and a synthesized two-member `.ttc`; the browser smoke test gains
    `exact-otf` and `exact-vf` cases — both OTS-accepted in headless
    Chromium with palette-correct pixels, the check that caught the last
    shipped-on-sight regression.

## 0.2.3

- **browser smoke test** (`pnpm smoke`, `scripts/browser-smoke.mjs`, new
  `browser-smoke.yml` CI job): builds fonts with the CLI, loads them in
  headless Chromium through the real FontFace/OTS path, renders the
  sample inside a `white-space: pre` element (the context real code
  blocks use) and samples rendered pixels against the Dark+ palette —
  the check the rustybuzz oracle cannot make. It caught a
  shipped-on-sight bug on its first run (the composite-flag rejection
  below). Covers exact mode, exact mode with --ligatures, and generic
  rust.

- **fix: exact-mode subsets were browser-rejected since 0.2.2** — the
  composite walker followed the wrong flag bit layout (MORE_COMPONENTS
  0x0010 instead of 0x0020; scale flags 0x0020/0x0040 instead of
  0x0040/0x0080 — every implementation numbers them the Apple way:
  OTS/Chrome, FreeType, fonttools). Multi-component composites were
  truncated at the first component, so the subset dropped real
  components and left un-remapped gids pointing past the subset
  ("Invalid glyph id used in composite glyph" at OTS; whole-font
  rejection, silent fallback to plain monospace). The fixture carries
  385 apple-layout composites; the rustybuzz oracle never walks
  outlines, so e2e stayed green. **Upgraders: rebuild any 0.2.2/0.2.3
  exact-mode font.**

- **reverted: the "coloring stopped at spaces in browsers" fix was
  wrong** — an earlier 0.2.3 entry here (since removed) claimed browsers
  split shaping runs at spaces and lost cross-space coloring, based on
  canvas/span probes. Neither was representative: DOM rendering
  carries spaces through shaping in every `white-space` mode
  (pre/normal/pre-wrap — the original "normal breaks it" probe text
  started with a space, which CSS strips at line start, so the rule's
  backtrack glyph never existed), and cross-space contextual rules
  apply everywhere in the DOM. Only `canvas fillText` splits shaping at
  spaces. Every released version's comments/strings colored correctly
  in real pages all along; the space-as-barrier change caused
  widespread false degradation and has been reverted (degradation on
  the demo site: 65 occurrences → zero). The smoke test renders in a
  real DOM element so the oracle measures what pages actually do.

- **browser smoke test** (`pnpm smoke`, `scripts/browser-smoke.mjs`, new
  `browser-smoke.yml` CI job): builds fonts with the CLI, loads them in
  headless Chromium through the real FontFace/OTS path, renders the
  sample inside a `white-space: pre` element (the context real code
  blocks use) and samples rendered pixels against the Dark+ palette —
  the check the rustybuzz oracle cannot make. It caught a
  shipped-on-sight bug on its first run (the composite-flag rejection
  below). Covers exact mode, exact mode with --ligatures, and generic
  rust.

- **fix: exact-mode subsets were browser-rejected since 0.2.2** — the
  composite walker followed the wrong flag bit layout (MORE_COMPONENTS
  0x0010 instead of 0x0020; scale flags 0x0020/0x0040 instead of
  0x0040/0x0080 — every implementation numbers them the Apple way:
  OTS/Chrome, FreeType, fonttools). Multi-component composites were
  truncated at the first component, so the subset dropped real
  components and left un-remapped gids pointing past the subset
  ("Invalid glyph id used in composite glyph" at OTS; whole-font
  rejection, silent fallback to plain monospace). The fixture carries
  385 apple-layout composites; the rustybuzz oracle never walks
  outlines, so e2e stayed green. **Upgraders: rebuild any 0.2.2/0.2.3
  exact-mode font.**

- **fix: coloring stopped at every space in real browsers** — Blink
  shapes space-delimited sub-runs, so no contextual rule can anchor
  across a space (verified with a minimal GSUB probe: backtrack on the
  space class never fires in Chromium; intra-word backtrack does).
  Rules anchored across spaces never matched, and colored spans
  containing spaces lost everything after the first one (strings/
  comments colored only their first word) while the rustybuzz oracle —
  shaping the whole text as one run — reported green. Run-splitting
  chars (space, tab, Zs, U+3000) are now hard separators in exact mode:
  occurrences split at them, rule windows stop at them, and they carry
  no color (no visible ink — visually identical). Duplicates that can
  only be told apart across a space now degrade with warnings, which is
  exactly what a browser could ever distinguish. Generic mode still
  anchors keywords/numbers across spaces — its engine needs a revision
  for run-start anchoring (known limitation below).

- **exact mode: `--ligatures` (node `ligatures: true`, plugins
  `ligatures: true`)** — preserve the source font's ligatures. The build
  now *shapes* each block with the source font (rustybuzz, the same
  algorithm browsers run) and re-materializes every substitution the
  shaper performed as one more minimal-window rule: the pristine per-char
  glyphs become the shaped glyph through a nested single/ligature
  substitution, audited globally like every other rule. This is the
  same "content is known at build time" trick applied to shaping: no
  source GSUB ships, nothing interferes with the coloring, and the
  subset stays minimal (ligature glyphs and rule inputs simply join the
  keep-set). A site whose member cells color uniformly restores the
  ligature glyph itself and colors it like any base glyph (one glyph,
  one color); a site with mixed colors degrades to plain per-char text
  so every character keeps its color (colors win, warning emitted).
  Ordering inside `calt` mirrors the shaper: restore rules run first —
  lookahead compiled against the pristine stream, backtrack against the
  restored one — then the coloring rules run entirely against the
  restored stream. The build-time oracle now checks shaping units
  (cluster spans) instead of per-char glyphs; the standalone `verify`
  rebuilds color intents without the source font, so it checks them
  against the font's own COLR claims (uncolored ligature outputs remain
  trusted from the build there). Opt-in and exact-mode only — generic
  mode's font must shape arbitrary text and keeps dropping the source
  layout tables. On the JBM fixture `!=`/`->`/`<=` are per-char
  contextual alternates (not n:1 ligatures) and are restored equally;
  the e2e suite covers true ligatures through an injected-LigatureSubst
  fixture: colored, uncolored, and mixed-color fates.


- **fix: generic-mode fonts were rejected by browsers since 0.2.2** (the
  OTS console spam). The non-subset glyf path in `assemble` prefixed the
  loca offsets twice (its own leading 0 plus the source loca's), shifting
  every source glyph one gid up: composite component gids inside the
  verbatim glyf then pointed at the wrong glyphs — including
  self-references — and browsers refused the whole font at OTS (console:
  "Component depth exceeds maxp maxComponentDepth", climbing limits;
  pages silently fell back to plain monospace — text readable, zero
  highlighting). The rustybuzz oracle stayed green because it only diffs
  glyph ids and never walks outlines, and exact-mode fonts were immune
  (the subset path builds its own offsets). Fixed by skipping the source
  loca's leading 0; a regression test now locks source glyphs
  byte-identical at the same gids, the loca entry count, and a
  composite-cycle walk — it fails on the old bytes. **Upgraders: any
  `--generic` font built by 0.2.2 is broken in browsers and should be
  rebuilt** (delete the `.chromaglyph-cache`; the version bump rotates
  the hash). Verified end-to-end: all eight generic demo fonts load
  (`document.fonts.check`), colors render, and a vision pass reads the
  exact sample text off the pixels.

- **generic mode: C++** (`--generic cpp` / `c++` / `cxx`, node
  `buildFont({generic: 'cpp'})`): C's lexical core plus
  class/template/namespace keyword groups — control flow, storage types
  (incl. `auto`, `decltype`, `typename`, the `char8_t`/`char16_t`/
  `char32_t` family), access modifiers and qualifiers (`public`,
  `constexpr`, `override`, `final`, …), `new`/`delete`/`this`/
  `namespace`/`template`/`operator`, preprocessor directive words (same
  trade-off as C), `true`/`false`/`nullptr`/`NULL`, numbers. Two C++
  constructs are deliberately not modeled (a glyph FSM cannot see them,
  same decision class as Python's f-strings): raw strings `R"(...)"` —
  `R` is a plain identifier, so the quote behaves like a normal escaped
  string (visually right unless the raw body contains a bare quote,
  whose closer delimiter is user-chosen and unknowable to the font) —
  and digit separators `1'000`, where the `'` opens a char-literal
  state like any other quote. 404 rules / 18 slots on the fixture font;
  oracle green over curated + spec-derived samples plus a 19-piece
  adversarial suite.

- **engine: keywords with embedded digits work** — found by the C++
  oracle the moment `char8_t` entered a keyword list (the first
  digit-containing keyword in any spec): glyph classification sent
  digits to the number class before consulting keyword-letter classes,
  so the keyword rule's input sequence could never match what the
  shaper sees (`char8_t` stayed plain while the reference lexer colored
  it — oracle red, loudly). Keyword letters are now classified before
  the digit test, and keyword-embedded digits' classes also open the
  number state after a boundary (`8;` colors; `x8` stays plain;
  `char8_t` colors whole). No behavior change for letter-only keywords.

## 0.2.2 (2026-10-17)

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
