import DefaultTheme from 'vitepress/theme'
import { syncChromaglyphPalette } from './chromaglyph-palette'

export default {
  extends: DefaultTheme,
  enhanceApp() {
    if (typeof window !== 'undefined') syncChromaglyphPalette()
  },
}
