<script setup lang="ts">
import { ElButton } from 'element-plus'
import type { Submission } from '../api/types'
import {
  findingCategories,
  reviewErrors,
  reviewStatuses,
} from '../utils/reviewPresentation'
import { submissionDateFormat as dateFormat } from '../utils/submissionPresentation'
import RequestFeedback from './RequestFeedback.vue'
import 'element-plus/es/components/button/style/css'

defineProps<{
  submission: Submission
  sending: boolean
  canStart: boolean
  refreshRequired: boolean
  error: unknown
}>()
defineEmits<{ start: []; refresh: [] }>()
</script>

<template>
  <section class="surface result-review" aria-labelledby="review-title">
    <div class="panel-heading review-heading">
      <h2 id="review-title">代码审查</h2>
      <ElButton
        :loading="sending"
        :disabled="!canStart"
        @click="$emit('start')"
      >
        {{ submission.review ? '重新审查' : '启动审查' }}
      </ElButton>
    </div>
    <p
      class="result-status"
      role="status"
      :class="{
        'is-success': submission.review_status === 'PASS',
        'is-failure': submission.review_status === 'FAILED',
        'is-doubtful': submission.review_status === 'DOUBTFUL',
      }"
    >
      {{ reviewStatuses[submission.review_status] }}
    </p>
    <p class="result-note">
      代码审查独立于机器判题结论。重新审查接纳后会替换上次结果。
    </p>
    <p v-if="submission.review_status === 'PENDING'" class="result-note">
      等待或正在审查。
    </p>
    <p v-else-if="submission.state !== 'finished'" class="result-note">
      只有机器判题已完成的提交可以启动审查。
    </p>
    <p v-else-if="submission.review_status === 'DISABLED'" class="result-note">
      自动审查未启用或未达到触发门槛，仍可手动启动。
    </p>
    <p v-if="refreshRequired" class="stale-notice" role="status">
      需要确认最新审查状态；查询成功前不能再次启动。查询失败时请刷新结果。
    </p>
    <div v-if="error" class="review-request-feedback">
      <RequestFeedback
        :error="error"
        operation="review"
        retry-text="刷新审查状态"
        @retry="$emit('refresh')"
      />
    </div>
    <template v-if="submission.review">
      <dl class="result-facts review-facts">
        <div>
          <dt>触发方式</dt>
          <dd>
            {{ submission.review.trigger === 'auto' ? '自动审查' : '手动审查' }}
          </dd>
        </div>
        <div>
          <dt>请求时间</dt>
          <dd>{{ dateFormat.format(submission.review.requested_at_ms) }}</dd>
        </div>
        <div>
          <dt>完成时间</dt>
          <dd>
            {{
              submission.review.completed_at_ms === null
                ? '尚未完成'
                : dateFormat.format(submission.review.completed_at_ms)
            }}
          </dd>
        </div>
        <div>
          <dt>模型</dt>
          <dd>{{ submission.review.model }}</dd>
        </div>
      </dl>
      <template v-if="submission.review.result">
        <h3>结论摘要</h3>
        <p class="review-text">{{ submission.review.result.summary }}</p>
        <ol
          v-if="submission.review.result.findings.length"
          class="review-findings"
        >
          <li
            v-for="(finding, index) in submission.review.result.findings"
            :key="index"
          >
            <h3>
              {{ findingCategories[finding.category] }} · 第
              {{ finding.line_start }}–{{ finding.line_end }} 行（含首尾）
            </h3>
            <h4>具体证据</h4>
            <p class="review-text">{{ finding.evidence }}</p>
            <h4>可能的正常解释</h4>
            <p class="review-text">{{ finding.normal_explanation }}</p>
          </li>
        </ol>
      </template>
      <div v-if="submission.review.error" class="review-failure" role="status">
        <p>{{ reviewErrors[submission.review.error.code] }}</p>
        <p class="failure-code">失败代码：{{ submission.review.error.code }}</p>
        <p class="result-note">
          本次没有有效审查结论，可在服务可用后手动重新审查。
        </p>
      </div>
    </template>
  </section>
</template>
