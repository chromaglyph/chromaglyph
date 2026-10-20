<script setup lang="ts">
// The count and collection version live in a component because markdown
// pages cannot import the generated JSON (their relative imports would
// have to survive VitePress's temp-file flattening).
import data from '../../themes.data.json'

withDefaults(defineProps<{ themesLabel?: string; versionLabel?: string }>(), {
  themesLabel: 'themes',
  versionLabel: 'collection version',
})
</script>

<template>
  <p class="ts">
    <strong>{{ data.themes.length }}</strong> {{ themesLabel }} ·
    {{ versionLabel }} <code>{{ data.version }}</code>
  </p>
</template>

<style scoped>
.ts code {
  font-size: 0.9em;
}
</style>
