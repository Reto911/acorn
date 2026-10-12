<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  title: string
  base64: string
  truncated: boolean
}>()
const text = computed(() =>
  new TextDecoder('utf-8').decode(
    Uint8Array.from(atob(props.base64), (character) => character.charCodeAt(0)),
  ),
)
</script>

<template>
  <section class="output-block" :aria-label="title">
    <h3>{{ title }}</h3>
    <p class="muted">按 UTF-8 展示，无法解码的字节显示为替换字符。</p>
    <p v-if="truncated" class="output-truncated">输出已截断</p>
    <p v-if="text.length === 0" class="muted">输出为空</p>
    <pre class="diagnostic-text" tabindex="0" :aria-label="title">{{
      text
    }}</pre>
  </section>
</template>
