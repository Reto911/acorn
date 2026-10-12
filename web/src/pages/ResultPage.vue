<script setup lang="ts">
import { ElButton } from 'element-plus'
import { RouterLink, useRoute } from 'vue-router'
import { getSubmission } from '../api'
import FixedProblemStatement from '../components/FixedProblemStatement.vue'
import RequestFeedback from '../components/RequestFeedback.vue'
import SubmissionOutcome from '../components/SubmissionOutcome.vue'
import { useSubmissionPolling } from '../composables/useSubmissionPolling'
import 'element-plus/es/components/button/style/css'

const route = useRoute()
const {
  data: submission,
  error,
  loading,
  hidden,
  terminal,
  refresh,
} = useSubmissionPolling(
  () => String(route.params.submissionId),
  (id, signal) => getSubmission(id, { signal, polling: true }),
)
const dateFormat = new Intl.DateTimeFormat('zh-CN', {
  dateStyle: 'medium',
  timeStyle: 'medium',
})
const timeZone = dateFormat.resolvedOptions().timeZone
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
    <div v-if="!submission || error" class="surface result-feedback">
      <RequestFeedback
        :loading="loading"
        loading-text="正在读取提交结果…"
        :error="error"
        @retry="refresh"
      />
      <p v-if="submission && error" class="stale-notice" role="status">
        查询已暂停。以下为最近成功取得的状态，可能已过期；请手动重试。
      </p>
    </div>
    <div v-if="submission" class="result-content">
      <section class="surface result-identity" aria-label="提交信息">
        <div class="result-navigation">
          <RouterLink
            class="text-link"
            :to="`/problems/${submission.problem_id}`"
            >返回答题</RouterLink
          >
          <RouterLink
            class="text-link"
            :to="{
              path: '/submissions',
              query: { problem_id: submission.problem_id },
            }"
            >本题提交记录</RouterLink
          >
          <ElButton
            :loading="loading"
            :disabled="loading || hidden"
            @click="refresh"
            >刷新结果</ElButton
          >
        </div>
        <dl class="result-facts">
          <div>
            <dt>题号 / 判题版本</dt>
            <dd>#{{ submission.problem_id }} / v{{ submission.version_no }}</dd>
          </div>
          <div>
            <dt>语言</dt>
            <dd>{{ submission.language_id === 'c' ? 'C' : 'Python 3' }}</dd>
          </div>
          <div>
            <dt>创建时间</dt>
            <dd>{{ dateFormat.format(submission.created_at_ms) }}</dd>
          </div>
          <div>
            <dt>终态保存时间</dt>
            <dd>
              {{
                submission.terminal_at_ms === null
                  ? '尚未产生'
                  : dateFormat.format(submission.terminal_at_ms)
              }}
            </dd>
          </div>
          <div v-if="submission.terminal_at_ms !== null">
            <dt>提交总耗时（含排队与调度）</dt>
            <dd>
              {{
                (submission.terminal_at_ms - submission.created_at_ms) / 1000
              }}
              s
            </dd>
          </div>
        </dl>
        <p class="result-note">
          时间按浏览器本地时区
          {{ timeZone }}
          显示。返回答题会打开最新题面和本地草稿，草稿并非本次提交的历史源码。
        </p>
      </section>
      <SubmissionOutcome :submission="submission" />
      <p v-if="!terminal && !error" class="result-note" role="status">
        {{
          hidden
            ? '标签页已隐藏，查询暂停。'
            : loading
              ? '正在刷新提交状态…'
              : '每次查询完成后等待 1 秒刷新状态。'
        }}
      </p>
      <FixedProblemStatement
        :key="`${submission.problem_id}:${submission.version_no}`"
        :problem-id="submission.problem_id"
        :version="submission.version_no"
        :language="submission.language_id"
      />
    </div>
  </section>
</template>
