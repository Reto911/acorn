<script setup lang="ts">
import { ElEmpty } from 'element-plus'
import 'element-plus/es/components/empty/style/css'
</script>

<template>
  <section aria-labelledby="page-title">
    <div class="page-heading">
      <h1 id="page-title">提交历史</h1>
      <p>查看后端配置用户的提交记录与判题结果。</p>
    </div>
    <div class="surface placeholder-panel">
      <ElEmpty description="提交历史功能正在接入" />
    </div>
  </section>
</template>
