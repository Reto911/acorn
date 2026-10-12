<script setup lang="ts">
import { onScopeDispose, ref, watch } from 'vue'
import { ElAlert, ElButton, ElMessageBox } from 'element-plus'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useSubmissionRequest } from '../composables/useSubmissionRequest'
import RequestFeedback from './RequestFeedback.vue'
import 'element-plus/es/components/alert/style/css'
import 'element-plus/es/components/button/style/css'
import 'element-plus/es/components/message-box/style/css'

const router = useRouter()
const route = useRoute()
const {
  pending,
  sending,
  error,
  restored,
  storageWarning,
  retry,
  abandonConfirmed,
} = useSubmissionRequest()
const confirming = ref(false)
let active = true
let pageGeneration = 0
watch(
  () => route.fullPath,
  () => {
    pageGeneration += 1
  },
  { flush: 'sync' },
)
onScopeDispose(() => {
  active = false
  if (confirming.value) ElMessageBox.close()
})

async function retryOriginal() {
  const originalPage = pageGeneration
  const result = await retry()
  if (result && active && pageGeneration === originalPage)
    await router.push({
      name: 'result',
      params: { submissionId: result.submission_id },
    })
}

async function confirmAbandon() {
  confirming.value = true
  try {
    await ElMessageBox.confirm(
      '原提交可能已经创建。放弃后将失去原请求的重试入口，不会撤销后端提交或判题。当前代码草稿会保留。',
      '放弃原提交请求？',
      {
        type: 'warning',
        confirmButtonText: '确认放弃',
        cancelButtonText: '保留原请求',
        distinguishCancelAndClose: true,
      },
    )
    if (active) abandonConfirmed()
  } catch (cause) {
    if (cause !== 'cancel' && cause !== 'close') throw cause
  } finally {
    confirming.value = false
  }
}
</script>

<template>
  <div v-if="pending || error || storageWarning" class="pending-submission">
    <ElAlert
      v-if="storageWarning"
      :title="storageWarning"
      type="warning"
      :closable="false"
      show-icon
    />
    <section
      v-if="pending"
      class="pending-request"
      aria-labelledby="pending-title"
    >
      <h2 id="pending-title">
        {{ restored ? '上次提交请求尚未完成' : '提交请求尚未完成' }}
      </h2>
      <p>
        原请求：题目 #{{ pending.body.problem_id }} ·
        {{ pending.body.language_id === 'c' ? 'C' : 'Python 3' }}。
        重试使用点击提交时的代码，后续编辑不会替换原内容。
      </p>
      <p>
        本标签页一次只保留一份待完成请求。完成或确认放弃后，才能开始另一份提交。
      </p>
      <p v-if="pending.submission_id" class="submission-id">
        原提交 ID：{{ pending.submission_id }}
      </p>
      <div class="pending-actions">
        <ElButton
          type="primary"
          :loading="sending"
          :disabled="sending || confirming"
          @click="retryOriginal"
        >
          重试原提交
        </ElButton>
        <ElButton :disabled="sending || confirming" @click="confirmAbandon">
          放弃原提交
        </ElButton>
        <RouterLink
          v-if="pending.submission_id"
          class="text-link"
          :to="{
            name: 'result',
            params: { submissionId: pending.submission_id },
          }"
        >
          查看原记录
        </RouterLink>
        <RouterLink class="text-link" to="/submissions"
          >查看提交历史</RouterLink
        >
      </div>
    </section>
    <RequestFeedback
      :loading="sending"
      loading-text="正在发送提交请求，请稍候…"
      :error="error"
      operation="create"
      :retryable="false"
    />
  </div>
</template>
