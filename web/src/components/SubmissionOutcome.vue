<script setup lang="ts">
import { computed } from 'vue'
import type { Submission } from '../api/types'
import {
  submissionStates as states,
  submissionVerdicts as verdicts,
} from '../utils/submissionPresentation'
import OutputText from './OutputText.vue'

const props = defineProps<{ submission: Submission }>()
const compile = computed(() =>
  props.submission.state === 'finished' && props.submission.verdict === 'CE'
    ? props.submission.terminal_details
    : null,
)
const failedCase = computed(() =>
  props.submission.state === 'finished' &&
  props.submission.verdict !== 'AC' &&
  props.submission.verdict !== 'CE'
    ? props.submission.terminal_details.failed_case
    : null,
)
const failure = computed(() =>
  props.submission.state === 'failed'
    ? props.submission.terminal_details
    : null,
)

function measurement(value: number | undefined, divisor: number, unit: string) {
  return value === undefined ? '未提供' : `${value / divisor} ${unit}`
}
</script>

<template>
  <section class="surface result-outcome" aria-labelledby="result-status-title">
    <h2 id="result-status-title">判题状态</h2>
    <p
      class="result-status"
      role="status"
      :class="{
        'is-success': submission.verdict === 'AC',
        'is-failure':
          submission.state === 'failed' ||
          (submission.state === 'finished' && submission.verdict !== 'AC'),
      }"
    >
      {{ states[submission.state] }}
      <strong v-if="submission.state === 'finished'">
        · {{ verdicts[submission.verdict] }}（{{ submission.verdict }}）</strong
      >
    </p>
    <p
      v-if="submission.state !== 'finished' && submission.state !== 'failed'"
      class="result-note"
    >
      等待后端更新此提交的状态。
    </p>
    <p v-if="submission.verdict === 'AC'">全部测试通过。</p>
    <OutputText
      v-if="compile"
      title="编译诊断"
      :base64="compile.compile_diagnostic_base64"
      :truncated="compile.compile_diagnostic_truncated"
    />
    <template v-if="failedCase">
      <h3>首个失败用例 #{{ failedCase.case_index }}</h3>
      <dl class="result-facts case-resources">
        <div>
          <dt>CPU 时间</dt>
          <dd>
            {{ measurement(failedCase.resources?.cpu_time_us, 1000, 'ms') }}
          </dd>
        </div>
        <div>
          <dt>墙钟时间</dt>
          <dd>
            {{ measurement(failedCase.resources?.wall_time_us, 1000, 'ms') }}
          </dd>
        </div>
        <div>
          <dt>峰值内存</dt>
          <dd>
            {{
              measurement(
                failedCase.resources?.peak_memory_bytes,
                1048576,
                'MiB',
              )
            }}
          </dd>
        </div>
        <div>
          <dt>退出码</dt>
          <dd>{{ failedCase.exit_code ?? '未提供' }}</dd>
        </div>
        <div>
          <dt>信号</dt>
          <dd>{{ failedCase.signal ?? '未提供' }}</dd>
        </div>
      </dl>
      <OutputText
        title="标准输出 stdout"
        :base64="failedCase.stdout_base64"
        :truncated="failedCase.stdout_truncated"
      />
      <OutputText
        title="标准错误 stderr"
        :base64="failedCase.stderr_base64"
        :truncated="failedCase.stderr_truncated"
      />
    </template>
    <template v-if="failure">
      <p class="failure-code">失败代码：{{ failure.code }}</p>
      <p v-if="failure.code === 'SchedulerInterrupted'" class="result-note">
        后端重启导致此次执行中断。此提交已结束，不会自动重新提交。
      </p>
      <h3>执行失败诊断</h3>
      <pre class="diagnostic-text" tabindex="0" aria-label="执行失败诊断">{{
        failure.diagnostic
      }}</pre>
    </template>
  </section>
</template>
