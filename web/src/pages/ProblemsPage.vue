<script setup lang="ts">
import { ElEmpty } from 'element-plus'
import 'element-plus/es/components/empty/style/css'
</script>

<template>
  <section aria-labelledby="page-title">
    <div class="page-heading">
      <h1 id="page-title">题目</h1>
      <p>从一道题目开始，编写代码并查看判题结果。</p>
    </div>
    <div class="surface placeholder-panel">
      <ElEmpty description="题目浏览功能正在接入" />
    </div>
  </section>
</template>
