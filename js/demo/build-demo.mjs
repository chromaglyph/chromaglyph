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
  rust: `use std::fmt; // "use" at line start stays plain: no glyph to anchor on
// a comment
/* one-line block */ fn demo() { // fn AFTER a space anchors fine
    let s = "fn // let";
    let esc = "quote: \\" backslash: \\\\";
    let n = 3.14;
    let m = 1.max(2);
    /* multi-line comments color
       their opening line only */ let x = 1;
}`,
  javascript: `// config (top-level keywords at column 0 stay plain: no anchor)
function demo() {
    const name = 'chromaglyph';
    let tmpl = \`one-line template \${name}\`;
    let empty = ""; let mixed = 'a"b';
    let q = 'it\\'s'; let r = "a\\"b";
    if ($use) return 42;
    let multi = \`line one only,
plain after the newline\`;
}`,
  python: `"""One-line docstring."""
def main(): # def at line start: no anchor
    msg = "hi"
    # comment
    s = f"interp {x}"
    u = "esc: \\" ok"
    t = '''single triple "with dq" inside'''
    return 3.14`,
  go: `// config
package main // keywords at column 0 stay plain (no anchor glyph)
func demo() error {
    for i := range items { // for/range after spaces: colored
        if err := check(i); err != nil {
            return err // return colored; identifiers/operators stay plain
        }
        s := "plain" + \`raw string\`
        r := 'x'
        n := 3.14
    }
    return nil
}`,
  java: `// config
public class Demo { // public at column 0 stays plain; class is teal
    static final String NAME = "value"; // static/final blue; String is an
    // identifier — white in pure syntax highlighting (no language server)
    char q = '\\''; char c = 'x';
    double n = 3.14;
    int len(String s) {
        if (s == null || s.isEmpty()) return 0;
        return s.length();
    }
}`,
  typescript: `// TypeScript: top-level keywords stay plain (column 0, no anchor);
namespace Demo { // namespace too — but indented keywords color below
    interface User {
        readonly id: number
        name: string // string is at line end: no lookahead glyph
        tags?: string[]
    }
    export function greet(user: User): string {
        const { name, tags = [] } = user
        return \`hi \${name.toUpperCase()} bye [\${tags.join(', ')}]\`
        // interpolation: \${ and } keep the string color, the expression is plain
    }
}`,
  c: `// build config (top-level keywords at column 0 stay plain: no anchor)
#include <stdio.h> // include colors purple; the # glyph stays plain
#define MAX 42
int main(void) { // int AFTER a space anchors fine
    char *msg = "hi \\"chromaglyph\\"";
    char q = '\\''; char c = 'x';
    if (msg == NULL) return 1; // NULL blue, msg an identifier (plain)
    for (int i = 0; i < MAX; i++) printf("%d\\n", i); // numbers color
    return 0; /* block comment colors to its closer on this line */
}`,
  cpp: `// C++: keywords can carry digits — char8_t colors whole (8 included)
#include <vector>
namespace demo { // namespace at column 0 stays plain (no anchor glyph)
template <typename T> class Stack { // template/typename/class blue
  public: // access modifiers blue
    constexpr static std::size_t MAX = 64; // 64 colors as a number
    void push(const T &value) { items[top_++] = value; }
    auto size() const noexcept { return top_; }
  private:
    std::vector<T> items;
    char8_t tag = 'x'; char16_t w = 'w';
};
} // namespace demo
int run() {
    auto *s = new demo::Stack<int>(); // new blue, auto/indented keywords color
    if (s == nullptr) return 0; // nullptr blue
    delete s;
    return sizeof(int) * 2; // numbers color
}`,
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
  @media (prefers-color-scheme: light) {
    body { background: #ffffff; color: #1e1e1e; }
    pre { background: #ffffff; }
    ${sections.map((s) => `pre.${s.lang} { font-palette: --${s.family}-light; }`).join('\n    ')}
  }
</style>
${sections.map((s) => `<style>\n${s.css}\n</style>`).join('\n')}
<h1>chromaglyph × generic mode</h1>
<p>Eight fonts, zero compiled-in content: each font highlights ANY text of
its language — keywords, comments, strings, numbers — via a synthesized
state machine. Browsers shape every source line as its own run, so the
highlighting is line-scoped: a keyword colors when the glyphs beside it
live in the font (indent it one space and it colors; at column 0 or the
very end of a line it stays plain), and multi-line states (block comments,
template literals, docstrings) color their opening line only — the
continuation lines start a fresh, plain run. Identifiers and operators
are never colored — that is semantic-highlighting territory, which the
exact mode's tree-sitter layer covers.</p>
${sections
  .map((s) => `<h2>${s.lang}</h2>\n<pre class="${s.lang}">${s.code.replace(/&/g, '&amp;').replace(/</g, '&lt;')}</pre>`)
  .join('\n')}
`
writeFileSync(resolve(here, 'out/generic.html'), genericPage)
console.log('wrote out/generic.html')
