<script setup lang="ts">
// License attribution for every builtin theme (MIT and Apache-2.0 both
// require it). Column labels are props so the zh page can translate them.
import data from '../../themes.data.json'

withDefaults(
  defineProps<{
    themeLabel?: string
    licenseLabel?: string
    sourceLabel?: string
    upstreamLabel?: string
  }>(),
  { themeLabel: 'Theme', licenseLabel: 'License', sourceLabel: 'Source', upstreamLabel: 'upstream' },
)
</script>

<template>
  <table class="tc">
    <thead>
      <tr>
        <th>{{ themeLabel }}</th>
        <th>{{ licenseLabel }}</th>
        <th>{{ sourceLabel }}</th>
      </tr>
    </thead>
    <tbody>
      <tr v-for="t in data.themes" :key="t.name">
        <td>{{ t.displayName }} <code>{{ t.name }}</code></td>
        <td><a :href="t.licenseUrl" target="_blank" rel="noopener">{{ t.license }}</a></td>
        <td><a :href="t.source" target="_blank" rel="noopener">{{ upstreamLabel }} ↗</a></td>
      </tr>
    </tbody>
  </table>
</template>

<style scoped>
.tc code {
  font-size: 12px;
  color: var(--vp-c-text-2);
}
</style>
