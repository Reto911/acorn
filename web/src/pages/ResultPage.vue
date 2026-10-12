<script setup lang="ts">
import { ElEmpty } from 'element-plus'
import { RouterLink, useRoute } from 'vue-router'
import 'element-plus/es/components/empty/style/css'

const route = useRoute()
</script>

<template>
  <section aria-labelledby="page-title">
    <div class="page-heading">
      <RouterLink class="back-link" to="/submissions"
        >← 返回提交历史</RouterLink
      >
      <h1 id="page-title">提交结果</h1>
      <p class="submission-id">提交 ID：{{ route.params.submissionId }}</p>
    </div>
    <div class="surface placeholder-panel">
      <ElEmpty description="判题结果查询功能正在接入" />
    </div>
  </section>
</template>
