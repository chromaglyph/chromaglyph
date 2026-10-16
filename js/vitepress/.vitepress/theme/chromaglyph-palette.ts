// VitePress's appearance toggle switches the `html.dark` class; the CSS
// injected by the chromaglyph plugin follows `prefers-color-scheme` (OS
// level), and its scope rule `[data-chromaglyph], [data-chromaglyph] code`
// paints the background on BOTH the <pre> and its <code> child — flipping
// only the <pre> leaves the code's dark background on top.
//
// This shim injects a stylesheet scoped to the html class instead:
// `html.dark`-prefixed selectors out-specify the plugin CSS (including its
// media query) and flip pre and code together — a palette flip with zero
// reflow. Palette names are read at runtime from the injected
// `@font-palette-values` rules (rule order = theme order:
// themes[0]=dark-plus, themes[1]=light-plus), no hash hardcoding.
const PALETTE_RE = /@font-palette-values\s+(--[\w-]+)/g

// dark-plus / light-plus backgrounds (VS Code theme-defaults)
const BACKGROUNDS = { dark: '#1e1e1e', light: '#ffffff' } as const

function chromaglyphPalettes(): { dark: string; light: string } | null {
  const names: string[] = []
  for (const sheet of Array.from(document.styleSheets)) {
    let text: string
    try {
      text = Array.from(sheet.cssRules)
        .map((r) => r.cssText)
        .join('\n')
    } catch {
      continue // cross-origin stylesheet
    }
    for (const m of text.matchAll(PALETTE_RE)) names.push(m[1])
  }
  if (names.length < 2) return null
  return { dark: names[0], light: names[names.length - 1] }
}

let sheet: HTMLStyleElement | null = null

function sync() {
  const palettes = chromaglyphPalettes()
  if (!palettes) return
  if (!sheet) {
    sheet = document.createElement('style')
    document.head.appendChild(sheet)
  }
  // `html.dark A, B` does NOT distribute the prefix to B — every selector
  // in the comma list must be prefixed individually, otherwise the code
  // rule loses specificity to the plugin stylesheet loaded later
  const scope = (pseudo: string) =>
    `html${pseudo} [data-chromaglyph], html${pseudo} [data-chromaglyph] code`
  const css =
    `${scope('.dark')} { font-palette: ${palettes.dark}; background: ${BACKGROUNDS.dark}; }\n` +
    `${scope(':not(.dark)')} { font-palette: ${palettes.light}; background: ${BACKGROUNDS.light}; }`
  if (sheet.textContent !== css) sheet.textContent = css
}

export function syncChromaglyphPalette() {
  sync()
  // One font per page (content hash differs → palette names differ):
  // re-scan when SPA navigation / first mount brings a new data:
  // stylesheet. sync() dedupes by content, frequent firing is harmless.
  let scheduled = false
  const observer = new MutationObserver(() => {
    if (scheduled) return
    scheduled = true
    requestAnimationFrame(() => {
      scheduled = false
      sync()
    })
  })
  observer.observe(document.body, { childList: true, subtree: true })
}
