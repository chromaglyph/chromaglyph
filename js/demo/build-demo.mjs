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
