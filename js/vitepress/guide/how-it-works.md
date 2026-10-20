# How it works

```
tokenize+theme → compile → fontbuild → verify
```

1. **tokenize + theme**: syntect parses with real TextMate grammars, then
   resolves every character to a concrete color under each theme.
2. **compile** (the slot model): a slot is one deduplicated per-theme color
   vector. Every colored run becomes one chained context substitution rule
   whose context is the smallest window that uniquely anchors the
   occurrence, audited globally for false matches across all blocks.
3. **fontbuild**: source font tables are preserved byte-for-byte where
   possible; colored copies are zero-outline glyphs plus COLRv0 layer
   references to the base glyphs, one CPAL palette per theme.
4. **verify**: harfrust shapes the produced font in-process and the
   substituted glyph ids are compared character by character, then every
   palette is read back from CPAL and checked.

## Key trade-offs

- Glyph-less characters (newlines, emoji) are **hard barriers**: browser
  fallback pulls them out of this font's shaping run, so rule contexts
  never cross them.
- Because rules depend only on slots, **one GSUB serves every theme** —
  switching palettes causes zero reflow.
- Long tokens (comments, strings) are split into chained segments within
  the shaper's 64-glyph context budget.

The Python block below exercises multi-line structure — every newline is a
barrier and string coloring closes within the line:

```python
MAX_RETRIES = 3  # UPPER_CASE assignment = constant (semantic layer)

def fetch(url, *, retries=MAX_RETRIES):
    """GET with retries. A triple-quoted docstring never closes a ' quote."""
    for attempt in range(retries):
        try:
            return request("GET", url)
        except TimeoutError as exc:
            log.warning(f"attempt {attempt} failed: {exc}")
    raise ConnectionError(f"no answer after {retries} retries")
```

## The oracle

Every font passes an oracle before shipping: a reference lexer plus
hundreds of deterministic random samples derived from the language spec,
aligned glyph by glyph with the shaper's behavior.

```ts
// Semantic layer: definitions, call sites, parameters, properties and
// built-in types each get their own color slot
const registry = new Map<string, () => Promise<Response>>()

async function retry(fn: () => Promise<Response>, times: number) {
  for (let i = 0; i < times; i++) {
    try {
      return await fn()
    } catch (err) {
      if (i === times - 1) throw err
    }
  }
  throw new Error('unreachable')
}
```
