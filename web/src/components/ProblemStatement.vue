<script setup lang="ts">
import { computed } from 'vue'
import MarkdownIt from 'markdown-it'

const props = defineProps<{ statement: string }>()
// Keep the parser's link validation and escaping; do not enable raw HTML or plugins.
const markdown = new MarkdownIt({ html: false })
const html = computed(() => markdown.render(props.statement))
</script>

<template>
  <!-- Only parser-generated HTML is rendered; the statement is never a Vue template. -->
  <!-- eslint-disable-next-line vue/no-v-html -->
  <div class="problem-statement" v-html="html" />
</template>

<style scoped>
.problem-statement {
  padding: 24px;
  overflow-wrap: anywhere;
  font-size: 15px;
  line-height: 1.8;
}

.problem-statement :deep(> :first-child) {
  margin-top: 0;
}

.problem-statement :deep(> :last-child) {
  margin-bottom: 0;
}

.problem-statement :deep(h1),
.problem-statement :deep(h2),
.problem-statement :deep(h3),
.problem-statement :deep(h4),
.problem-statement :deep(h5),
.problem-statement :deep(h6) {
  margin: 24px 0 12px;
  font-weight: 600;
  line-height: 1.5;
}

.problem-statement :deep(h1) {
  font-size: 24px;
}

.problem-statement :deep(h2) {
  font-size: 20px;
}

.problem-statement :deep(h3) {
  font-size: 17px;
}

.problem-statement :deep(p),
.problem-statement :deep(ul),
.problem-statement :deep(ol),
.problem-statement :deep(pre),
.problem-statement :deep(blockquote) {
  margin: 12px 0;
}

.problem-statement :deep(ul),
.problem-statement :deep(ol) {
  padding-left: 24px;
}

.problem-statement :deep(a) {
  color: #426b52;
  text-decoration: underline;
  text-underline-offset: 3px;
}

.problem-statement :deep(code) {
  padding: 2px 5px;
  background: #f1f4ef;
  border-radius: 4px;
  font-family: Consolas, 'Liberation Mono', monospace;
  font-size: 0.9em;
}

.problem-statement :deep(pre) {
  max-width: 100%;
  overflow-x: auto;
  padding: 16px;
  background: #f6f8f5;
  border: 1px solid #e2e8e2;
  border-radius: 6px;
  line-height: 1.6;
}

.problem-statement :deep(pre code) {
  padding: 0;
  background: transparent;
}

.problem-statement :deep(blockquote) {
  padding-left: 16px;
  border-left: 3px solid #c6d3cb;
  color: #68756c;
}

.problem-statement :deep(img) {
  max-width: 100%;
  height: auto;
}
</style>
