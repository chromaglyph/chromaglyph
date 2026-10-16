// Renders a multilingual markdown page through the remark pipeline and
// writes a standalone HTML demo (font inlined as a data URI).
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { tmpdir } from 'node:os'
import { fileURLToPath } from 'node:url'
import { unified } from 'unified'
import remarkParse from 'remark-parse'
import remarkRehype from 'remark-rehype'
import rehypeDocument from 'rehype-document'
import rehypeStringify from 'rehype-stringify'
import remarkChromaglyph from '@chromaglyph/remark'
import MarkdownIt from 'markdown-it'
import markdownItChromaglyph from '@chromaglyph/markdown-it'

const here = dirname(fileURLToPath(import.meta.url))
const font = readFileSync(resolve(here, '../../crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf'))

const markdown = `# chromaglyph × remark

One font, three languages, zero \`<span>\`. The text below is plain —
highlighting lives inside the font. Copy & paste is clean.

\`\`\`rust
fn main() {
    let msg = "hello chromaglyph";
    let n = 42;
    println!("{msg} {n}");
}
\`\`\`

\`\`\`javascript
// the same words, different grammar
const msg = "hello chromaglyph";
let n = 42;
console.log(msg, n);
\`\`\`

\`\`\`python
def main():
    msg = "hello chromaglyph"
    n = 42
    print(f"{msg} {n}")
\`\`\`
`

const t0 = Date.now()
const file = await unified()
  .use(remarkParse)
  .use(remarkChromaglyph, { font, cacheDir: resolve(tmpdir(), 'chromaglyph-demo-cache') })
  .use(remarkRehype, { allowDangerousHtml: true })
  .use(rehypeDocument, { title: 'chromaglyph × remark' })
  .use(rehypeStringify, { allowDangerousHtml: true })
  .process(markdown)
const elapsed = Date.now() - t0

const info = file.data.chromaglyph
console.log(`built in ${elapsed}ms (cached: ${info?.cached}) hash=${info?.contentHash?.slice(0, 8)}`)
console.log(`themes: ${info?.themes.map((t) => t.name).join(', ')}`)

let html = String(file)
// demo styling: dark page, sized code blocks
html = html.replace('</head>', `<style>
  body { background: #1e1e1e; color: #d4d4d4; max-width: 46rem; margin: 3rem auto; padding: 0 1rem; }
  h1 { font-family: system-ui, sans-serif; }
  pre[data-chromaglyph] { padding: 1rem; border-radius: 8px; overflow-x: auto; font-size: 15px; line-height: 1.6; }
  p > code { color: #d16969; }
</style></head>`)

mkdirSync(resolve(here, 'out'), { recursive: true })
writeFileSync(resolve(here, 'out/index.html'), html)
console.log('wrote out/index.html', html.length, 'bytes')

// ---- markdown-it variant (the VitePress path) ----
const md = new MarkdownIt({ html: true })
md.use(markdownItChromaglyph, { font, cacheDir: resolve(tmpdir(), 'chromaglyph-demo-cache') })
const t2 = Date.now()
const mditHtml = md.render(markdown)
const elapsed2 = Date.now() - t2
console.log(`markdown-it: built in ${elapsed2}ms (cached: ${md.chromaglyph()?.cached})`)
const page = `<!doctype html><meta charset="utf-8"><title>chromaglyph × markdown-it</title>
<style>
  body { background: #1e1e1e; color: #d4d4d4; max-width: 46rem; margin: 3rem auto; padding: 0 1rem; }
  h1 { font-family: system-ui, sans-serif; }
  pre[data-chromaglyph] { padding: 1rem; border-radius: 8px; overflow-x: auto; font-size: 15px; line-height: 1.6; }
  p > code { color: #d16969; }
</style>
<h1>chromaglyph × markdown-it (VitePress path)</h1>
${mditHtml}`
writeFileSync(resolve(here, 'out/mdit.html'), page)
console.log('wrote out/mdit.html')

// ---- generic mode: one font per language, highlights ANY text ----
// Content is NOT compiled in: the font synthesizes a state machine, so
// these code blocks could be anything — swapped at runtime, even.
import { buildFont } from '@chromaglyph/node'
import { buildCssAssets } from '@chromaglyph/node/css'

const samples = {
  rust: `use std::fmt; // NOTE: "use" at buffer start stays plain (no glyph to anchor on)
// a comment
/* multi
   line
   block */
fn main() {
    let s = "fn // let";
    let n = 3.14;
    let m = 1.max(2);
}`,
  javascript: `// config
const name = 'chromaglyph';
let tmpl = \`multi
line \${name}\`;
let empty = ""; let mixed = 'a"b';
if ($use) return 42;`,
  python: `"""Module docstring
spans lines."""
def main():
    # comment
    s = f"hi {x}"
    t = '''single triple "with dq" inside'''
    return 3.14`,
}

const sections = []
for (const [lang, code] of Object.entries(samples)) {
  const result = buildFont({ font, themes: ['dark-plus', 'light-plus'], blocks: [], generic: lang })
  const { css } = buildCssAssets(result, { embed: true })
  const family = css.match(/font-family: '(chromaglyph-[0-9a-f]+)'/)[1]
  sections.push({ lang, code, css, family })
  console.log(`generic ${lang}: ${result.woff.length} bytes (hash ${result.contentHash.slice(0, 8)})`)
}

const genericPage = `<!doctype html><meta charset="utf-8"><title>chromaglyph × generic</title>
<style>
  body { background: #1e1e1e; color: #d4d4d4; max-width: 46rem; margin: 3rem auto; padding: 0 1rem; }
  h1, h2 { font-family: system-ui, sans-serif; }
  pre { padding: 1rem; border-radius: 8px; overflow-x: auto; font-size: 15px; line-height: 1.6; }
  ${sections.map((s) => `pre.${s.lang} { font-family: ${s.family}, monospace; }`).join('\n  ')}
</style>
${sections.map((s) => s.css).join('\n')}
<h1>chromaglyph × generic mode</h1>
<p>Three fonts, zero compiled-in content: each font highlights ANY text of
its language — keywords, comments, strings, numbers — via a synthesized
state machine. Line-start keywords, multi-line block comments, JS template
literals and Python docstrings all survive newlines (an injected zero-width
newline glyph keeps the shaping run whole).</p>
${sections
  .map((s) => `<h2>${s.lang}</h2>\n<pre class="${s.lang}">${s.code.replace(/&/g, '&amp;').replace(/</g, '&lt;')}</pre>`)
  .join('\n')}
`
writeFileSync(resolve(here, 'out/generic.html'), genericPage)
console.log('wrote out/generic.html')
