<script setup lang="ts">
import { ElEmpty } from 'element-plus'
import { RouterLink } from 'vue-router'
import 'element-plus/es/components/empty/style/css'
</script>

<template>
  <section class="surface not-found" aria-labelledby="page-title">
    <h1 id="page-title">404 · 页面不存在</h1>
    <ElEmpty description="请检查地址，或返回题目列表。">
      <RouterLink class="text-link" to="/problems">返回题目列表</RouterLink>
    </ElEmpty>
  </section>
</template>
