/**
 * Real-browser smoke test: the check the rustybuzz oracle cannot make.
 *
 * The 0.2.2 generic-mode regression shipped because rustybuzz only diffs
 * glyph ids — a font browsers refuse at OTS (broken loca/composites) was
 * perfectly green in-process. This script builds fonts with the CLI,
 * loads them in headless Chromium through the same FontFace/OTS path a
 * browser uses, and samples rendered pixels against the Dark+ palette:
 *
 *   1. document.fonts.check()  — the font survives OTS sanitization
 *   2. canvas pixel probes     — keywords/strings/comments actually paint
 *      in their theme colors (dark-plus: #569cd6 / #ce9178 / #6a9955)
 *
 * Three fonts cover the layout paths: exact mode (subset), exact mode
 * with --ligatures (restore rules + subset), and generic rust (full
 * charset, no subsetting). Run: `pnpm smoke` (or pass CHROMAGLYPH_BIN).
 */
import { spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chromium } from 'playwright'

const root = resolve(import.meta.dirname, '..')
const outDir = resolve(root, 'target/smoke')
const bin = process.env.CHROMAGLYPH_BIN
  ?? resolve(root, 'target/release/chromaglyph')
const font = resolve(root, 'crates/chromaglyph-core/tests/fixtures/JetBrainsMono-Regular.ttf')

const SAMPLE = `// chromaglyph browser smoke
fn main() {
    let msg = "hello browser";
    let n = 42;
    println!("{} {}", msg, n); // -> != <=
}
`


// Dark+ palette anchors the pixel probes (stable VS Code colors). Exact
// mode anchors keywords by content, so line-start `fn`/`let` color there;
// generic mode's FSM needs a boundary glyph on both sides of a keyword,
// so its probes cover strings/comments/numbers instead.
// probe floors: before the run-split fix, only the first space-free
// segment of a string/comment colored (457/117px); healthy full-span
// coloring is ~1.6KB/850px — the floor sits well above the broken value
const EXACT_PROBES = [
  { name: 'keyword blue', css: '#569cd6', min: 800 },
  { name: 'string orange', css: '#ce9178', min: 900 },
  { name: 'comment green', css: '#6a9955', min: 400 },
]
// generic mode: keywords/numbers need cross-space anchoring the browser
// cannot provide (runs split at spaces) — a known limitation pending an
// engine revision; strings/comments color within their first run
const GENERIC_PROBES = [
  { name: 'string orange', css: '#ce9178' },
  { name: 'comment green', css: '#6a9955' },
]

const CASES = [
  { id: 'exact', args: ['--input', resolve(root, 'examples/hello.rs')], probes: EXACT_PROBES },
  {
    id: 'exact-lig',
    args: ['--input', resolve(outDir, 'sample.rs'), '--ligatures'],
    text: SAMPLE,
    probes: EXACT_PROBES,
  },
  { id: 'generic', args: ['--generic', 'rust'], probes: GENERIC_PROBES },
]

function buildFont({ id, args, text }) {
  if (text !== undefined) writeFileSync(resolve(outDir, 'sample.rs'), text)
  const out = resolve(outDir, `${id}.woff`)
  const run = spawnSync(bin, ['build', '--font', font, ...args, '--output', out], {
    encoding: 'utf8',
  })
  if (run.status !== 0) {
    throw new Error(`chromaglyph build failed for ${id}:\n${run.stdout}\n${run.stderr}`)
  }
  return out
}

async function main() {
  mkdirSync(outDir, { recursive: true })

  // the probe page: loads the font, waits for OTS, then renders the sample
  // on a canvas at a size where colored pixel cores are unambiguous
  const pageHtml = (woffName) => `<!doctype html>
<meta charset="utf-8">
<style>
@font-face {
  font-family: 'chromaglyph';
  src: url('${woffName}') format('woff');
}
body { background: #1e1e1e; margin: 0; }
</style>
<canvas id="c" width="1400" height="400"></canvas>
<script>
// classic script: ES modules are blocked on file:// (opaque origin CORS)
const SAMPLE = ${JSON.stringify(SAMPLE)}
const canvas = document.getElementById('c')
const ctx = canvas.getContext('2d', { willReadFrequently: true })
window.probe = async () => {
  await document.fonts.load("48px chromaglyph")
  await document.fonts.ready
  const loaded = document.fonts.check('48px chromaglyph')
  ctx.fillStyle = '#1e1e1e'
  ctx.fillRect(0, 0, canvas.width, canvas.height)
  ctx.font = '48px chromaglyph, monospace'
  ctx.textBaseline = 'top'
  const lines = SAMPLE.split('\\n')
  for (const [i, line] of lines.entries()) ctx.fillText(line, 16, 16 + i * 60)
  return { loaded, data: ctx.getImageData(0, 0, canvas.width, canvas.height).data }
}
</script>`

  const browser = await chromium.launch()
  let failures = 0
  try {
    for (const case_ of CASES) {
      const woff = buildFont(case_)
      const html = resolve(outDir, `${case_.id}.html`)
      writeFileSync(html, pageHtml(`${case_.id}.woff`))
      const page = await browser.newPage()
      await page.goto(`file://${html}`)
      const { loaded, data } = await page.evaluate(() => window.probe())

      if (!loaded) {
        console.error(`FAIL ${case_.id}: the browser refused the font (OTS rejection)`)
        failures++
        await page.close()
        continue
      }
      const counts = case_.probes.map((probe) => {
        const [r, g, b] = [1, 3, 5].map((i) => parseInt(probe.css.slice(i, i + 2), 16))
        let n = 0
        for (let p = 0; p < data.length; p += 4) {
          if (
            Math.abs(data[p] - r) <= 32 &&
            Math.abs(data[p + 1] - g) <= 32 &&
            Math.abs(data[p + 2] - b) <= 32
          )
            n++
        }
        return { name: probe.name, n }
      })
      const missing = counts.filter((c) => c.n < (case_.probes.find((p) => p.name === c.name).min ?? 20))
      if (missing.length > 0) {
        console.error(
          `FAIL ${case_.id}: font loads but colors do not render: ` +
            missing.map((c) => `${c.name} (${c.n}px)`).join(', '),
        )
        failures++
      } else {
        console.log(
          `ok   ${case_.id}: OTS-accepted, ` +
            counts.map((c) => `${c.name} ${c.n}px`).join(', '),
        )
      }
      await page.close()
    }
  } finally {
    await browser.close()
  }
  if (failures > 0) process.exit(1)
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
