<script setup lang="ts">
import { defineAsyncComponent } from 'vue'
import { ElEmpty } from 'element-plus'
import { RouterLink, useRoute } from 'vue-router'
import 'element-plus/es/components/empty/style/css'

const route = useRoute()
const CodeEditor = defineAsyncComponent(
  () => import('../components/CodeEditor.vue'),
)
</script>

<template>
  <section aria-labelledby="page-title">
    <div class="page-heading">
      <RouterLink class="back-link" to="/problems">← 返回题目列表</RouterLink>
      <h1 id="page-title">题目 #{{ route.params.problemId }}</h1>
      <p>题面、语言选择与提交功能正在接入。</p>
    </div>
    <div class="solve-layout">
      <section class="surface" aria-labelledby="statement-title">
        <div class="panel-heading"><h2 id="statement-title">题面</h2></div>
        <ElEmpty description="题面尚未加载" />
      </section>
      <section class="surface editor-panel" aria-labelledby="editor-title">
        <div class="panel-heading">
          <h2 id="editor-title">代码编辑区</h2>
          <span class="muted">暂未开放编辑与提交</span>
        </div>
        <CodeEditor />
      </section>
    </div>
  </section>
</template>
