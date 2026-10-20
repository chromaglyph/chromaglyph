// https://vitepress.dev/guide/custom-theme
import { h } from 'vue'
import type { Theme } from 'vitepress'
import DefaultTheme from 'vitepress/theme'
import ThemeGallery from './ThemeGallery.vue'
import ThemeCredits from './ThemeCredits.vue'
import ThemeStats from './ThemeStats.vue'
import './style.css'

export default {
  extends: DefaultTheme,
  Layout: () => {
    return h(DefaultTheme.Layout, null, {
      // https://vitepress.dev/guide/extending-default-theme#layout-slots
    })
  },
  enhanceApp({ app }) {
    app.component('ThemeGallery', ThemeGallery)
    app.component('ThemeCredits', ThemeCredits)
    app.component('ThemeStats', ThemeStats)
  },
} satisfies Theme
