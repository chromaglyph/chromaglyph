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
 *   2. <pre> pixel probes      — keywords/strings/comments/numbers paint
 *      in their theme colors (dark-plus: #569cd6 / #ce9178 / #6a9955 /
 *      #b5cea8)
 *
 * Rendering happens inside a `white-space: pre` element — the context of
 * every real code block. That distinction matters: Blink splits shaping
 * runs at spaces only in wrappable text (white-space: normal/pre-wrap,
 * and canvas fillText); in pre-formatted text cross-space contextual
 * rules apply, which is what chromaglyph's rules rely on. Measuring in
 * canvas would under-color on purpose.
 *
 * Six fonts cover the layout paths: exact mode (subset), exact mode
 * with --ligatures (restore rules + subset), generic rust (full charset,
 * no subsetting), exact mode from a CFF/OTF source (CharStrings
 * surgery + subset), exact mode from a variable TrueType source (pinned
 * to the default instance, subset) and generic html (the markup tag
 * state machine: tag names probe as keyword blue, attribute values as
 * string orange, comments as comment green). Two more run the exact and
 * generic paths through the WOFF2 container (Brotli) — OTS sanitizes
 * that path separately from WOFF1's zlib. Run: `pnpm smoke` (or pass
 * CHROMAGLYPH_BIN).
 */
import { spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chromium } from 'playwright'

const root = resolve(import.meta.dirname, '..')
const outDir = resolve(root, 'target/smoke')
const bin = process.env.CHROMAGLYPH_BIN
  ?? resolve(root, 'target/release/chromaglyph')
const fixtures = resolve(root, 'crates/chromaglyph-core/tests/fixtures')
const font = resolve(fixtures, 'JetBrainsMono-Regular.ttf')

// note: generic-mode keywords need a preceding in-line boundary glyph
// (line starts have none — a designed limitation of the FSM), so the
// sample carries mid-line keywords too
const SAMPLE = `// chromaglyph browser smoke
fn main() {
    let msg = "hello browser"; let n = 42;
    println!("{} {}", msg, n); // -> != <=
}
`

const PROBES = [
  { name: 'keyword blue', css: '#569cd6' },
  { name: 'string orange', css: '#ce9178' },
  { name: 'comment green', css: '#6a9955' },
  { name: 'number green', css: '#b5cea8' },
]

const HTML_SAMPLE = `<div class="hero">
  <!-- html smoke: tag machine -->
  <a href="https://example.io/x?a=1">link</a> &amp; more
</div>
`

const CASES = [
  { id: 'exact', args: ['--input', resolve(root, 'examples/hello.rs')] },
  {
    id: 'exact-lig',
    args: ['--input', resolve(outDir, 'sample.rs'), '--ligatures'],
    text: SAMPLE,
  },
  { id: 'generic', args: ['--generic', 'rust'] },
  {
    id: 'generic-html',
    args: ['--generic', 'html'],
    page: HTML_SAMPLE,
    probes: [
      { name: 'tag blue', css: '#569cd6' },
      { name: 'attr value orange', css: '#ce9178' },
      { name: 'comment green', css: '#6a9955' },
    ],
  },
  {
    id: 'exact-otf',
    font: resolve(fixtures, 'SourceCodePro-Regular.otf'),
    args: ['--input', resolve(outDir, 'sample.rs')],
    text: SAMPLE,
  },
  {
    id: 'exact-vf',
    font: resolve(fixtures, 'JetBrainsMono-wght-VF.ttf'),
    args: ['--input', resolve(outDir, 'sample.rs')],
    text: SAMPLE,
  },
  {
    id: 'exact-woff2',
    args: ['--input', resolve(root, 'examples/hello.rs')],
    ext: '.woff2',
  },
  {
    id: 'generic-woff2',
    args: ['--generic', 'rust'],
    ext: '.woff2',
  },
]

function buildFont({ id, font: src = font, args, text, ext = '.woff' }) {
  if (text !== undefined) writeFileSync(resolve(outDir, 'sample.rs'), text)
  const out = resolve(outDir, `${id}${ext}`)
  const run = spawnSync(bin, ['build', '--font', src, ...args, '--output', out], {
    encoding: 'utf8',
  })
  if (run.status !== 0) {
    throw new Error(`chromaglyph build failed for ${id}:\n${run.stdout}\n${run.stderr}`)
  }
  return out
}

async function main() {
  mkdirSync(outDir, { recursive: true })

  const browser = await chromium.launch()
  let failures = 0
  try {
    for (const case_ of CASES) {
      const woff = buildFont(case_)
      const page = await browser.newPage({ viewport: { width: 1400, height: 400 } })
      const pageText = case_.page ?? SAMPLE
      const body = `<style>body{margin:0;background:#1e1e1e;font-size:40px}pre{white-space:pre;margin:0}</style><pre id="a">${pageText.replace(/&/g, '&amp;').replace(/</g, '&lt;')}</pre>`
      // encodeURIComponent keeps newlines (and #, &) alive through the
      // data: URL — raw newlines are stripped, &#10; is cut at the #
      await page.goto('data:text/html,' + encodeURIComponent(body))
      // the container follows --output's extension; OTS handles the
      // woff2 (Brotli) path separately from woff1's zlib
      const fontBytes = readFileSync(woff)
      const mime = fontBytes.subarray(0, 4).equals(Buffer.from('wOF2')) ? 'font/woff2' : 'font/woff'
      const loaded = await page.evaluate(
        async ([mime, b64]) => {
          const face = new FontFace('chromaglyph', `url(data:${mime};base64,${b64})`)
          await face.load()
          document.fonts.add(face)
          document.getElementById('a').style.fontFamily = 'chromaglyph'
          await document.fonts.ready
          await new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(ok)))
          return document.fonts.check('40px chromaglyph')
        },
        [mime, fontBytes.toString('base64')],
      )

      if (!loaded) {
        console.error(`FAIL ${case_.id}: the browser refused the font (OTS rejection)`)
        failures++
        await page.close()
        continue
      }
      // pixel-probe the DOM rendering itself: screenshot the <pre>, decode
      // and count pixels near each palette color — the glyphs must come
      // from the font, laid out by the real text stack
      const shot = await page.screenshot()
      const probeList = case_.probes ?? PROBES
      const counts = await page.evaluate(
        (args) => {
          const [b64, probeList] = args
          const img = new Image()
          const done = new Promise((ok) => {
            img.onload = ok
            img.src = 'data:image/png;base64,' + b64
          })
          return done.then(() => {
            const c = document.createElement('canvas')
            c.width = img.width
            c.height = img.height
            const ctx = c.getContext('2d', { willReadFrequently: true })
            ctx.drawImage(img, 0, 0)
            const d = ctx.getImageData(0, 0, c.width, c.height).data
            return probeList.map(({ name, css }) => {
              const [r, g, b] = [1, 3, 5].map((i) => parseInt(css.slice(i, i + 2), 16))
              let n = 0
              for (let p = 0; p < d.length; p += 4) {
                if (
                  Math.abs(d[p] - r) <= 32 &&
                  Math.abs(d[p + 1] - g) <= 32 &&
                  Math.abs(d[p + 2] - b) <= 32
                )
                  n++
              }
              return { name, n }
            })
          })
        },
        [shot.toString('base64'), probeList],
      )

      const missing = counts.filter((c) => c.n < 20)
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
