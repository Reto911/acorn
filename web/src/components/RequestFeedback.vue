<script setup lang="ts">
import { computed } from 'vue'
import { ElAlert, ElButton, ElEmpty } from 'element-plus'
import { errorFeedback } from '../api/feedback'
import 'element-plus/es/components/alert/style/css'
import 'element-plus/es/components/button/style/css'
import 'element-plus/es/components/empty/style/css'

const props = withDefaults(
  defineProps<{
    loading?: boolean
    loadingText?: string
    error?: unknown
    operation?: 'query' | 'create' | 'review'
    empty?: boolean
    emptyText?: string
    emptyDescription?: string
    retryable?: boolean
    retryText?: string
  }>(),
  {
    error: undefined,
    loadingText: '正在加载，请稍候…',
    operation: 'query',
    emptyText: '暂无数据',
    emptyDescription: '',
    retryable: true,
    retryText: '重试',
  },
)
defineEmits<{ retry: [] }>()
const feedback = computed(() => errorFeedback(props.error, props.operation))
</script>

<template>
  <div v-if="loading" class="request-loading" role="status" aria-live="polite">
    {{ loadingText }}
  </div>
  <div v-else-if="error" class="request-error">
    <ElAlert :title="feedback.title" type="error" :closable="false" show-icon>
      <p>{{ feedback.description }}</p>
      <details v-if="feedback.code || feedback.detail" class="request-details">
        <summary>错误详情</summary>
        <p v-if="feedback.code">错误码：{{ feedback.code }}</p>
        <p v-if="feedback.detail">{{ feedback.detail }}</p>
      </details>
    </ElAlert>
    <ElButton v-if="retryable" class="feedback-action" @click="$emit('retry')">
      {{ retryText }}
    </ElButton>
  </div>
  <div v-else-if="empty" class="request-empty">
    <ElEmpty :description="emptyText" />
    <p v-if="emptyDescription">{{ emptyDescription }}</p>
  </div>
</template>
