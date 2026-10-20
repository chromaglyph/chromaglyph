<script setup lang="ts">
// Cards for the builtin themes of one kind. The preview is plain HTML
// painted with the theme's palette (resolved by the real highlighter at
// build time, see scripts/gen-themes.mjs) — not the font — so the page stays cheap
// and every theme can be compared before building one.
import { computed } from 'vue'
import type { BuiltinThemeInfo } from '@chromaglyph/node'
import data from '../../themes.data.json'

const props = defineProps<{
  type: 'dark' | 'light'
  sourceLabel?: string
}>()

const themes = computed(() => data.themes.filter((t) => t.type === props.type))

// one token = [text, scope]; `null` scope renders in the default foreground
type Token = [string, string | null]
const PREVIEW: Token[][] = [
  [['// colors live in the glyphs', 'comment.line']],
  [
    ['fn', 'storage.type'],
    [' ', null],
    ['greet', 'entity.name.function'],
    ['(', null],
    ['name', 'variable.other'],
    [': &', null],
    ['str', 'entity.name.type'],
    [') -> ', null],
    ['String', 'entity.name.type'],
    [' {', null],
  ],
  [
    ['    ', null],
    ['if', 'keyword.control'],
    [' ', null],
    ['name', 'variable.other'],
    ['.len() > ', null],
    ['42', 'constant.numeric'],
    [' {', null],
  ],
  [
    ['        ', null],
    ['return', 'keyword.control'],
    [' ', null],
    ['"hello"', 'string.quoted.double'],
    ['.into();', null],
  ],
  [['    }', null]],
  [['}', null]],
]

function color(t: BuiltinThemeInfo, scope: string | null) {
  return (scope && t.swatches[scope]) || t.foreground
}

function background(t: BuiltinThemeInfo) {
  return t.background ?? (t.type === 'dark' ? '#1e1e1e' : '#ffffff')
}
</script>

<template>
  <div class="tg-grid">
    <article v-for="t in themes" :key="t.name" class="tg-card">
      <header class="tg-head">
        <span class="tg-title">{{ t.displayName }}</span>
        <code class="tg-slug">{{ t.name }}</code>
      </header>
      <pre
        class="tg-preview"
        :style="{ background: background(t), color: t.foreground }"
      ><template v-for="(line, i) in PREVIEW" :key="i"><span
          v-for="([text, scope], j) in line"
          :key="j"
          :style="{ color: color(t, scope) }"
        >{{ text }}</span>{{ i < PREVIEW.length - 1 ? '\n' : '' }}</template></pre>
      <footer class="tg-foot">
        <a :href="t.licenseUrl" target="_blank" rel="noopener">{{ t.license }}</a>
        <a :href="t.source" target="_blank" rel="noopener">{{ sourceLabel ?? 'source' }} ↗</a>
      </footer>
    </article>
  </div>
</template>

<style scoped>
.tg-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 16px;
  margin: 16px 0 32px;
}
.tg-card {
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  overflow: hidden;
  background: var(--vp-c-bg-soft);
}
.tg-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 8px;
  padding: 10px 12px 8px;
}
.tg-title {
  font-weight: 600;
}
.tg-slug {
  font-size: 12px;
  color: var(--vp-c-text-2);
  background: transparent;
  padding: 0;
}
.tg-preview {
  margin: 0;
  padding: 12px 14px;
  font-family: var(--vp-font-family-mono);
  font-size: 12.5px;
  line-height: 1.5;
  overflow-x: auto;
  white-space: pre;
}
.tg-foot {
  display: flex;
  justify-content: space-between;
  padding: 8px 12px;
  font-size: 12px;
}
.tg-foot a {
  color: var(--vp-c-text-2);
  text-decoration: none;
}
.tg-foot a:hover {
  color: var(--vp-c-brand-1);
}
</style>
