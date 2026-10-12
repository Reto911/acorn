<script setup lang="ts">
import { computed } from 'vue'
import { getProblemVersion } from '../api'
import type { LanguageId } from '../api/types'
import { useQuery } from '../composables/useQuery'
import ProblemStatement from './ProblemStatement.vue'
import RequestFeedback from './RequestFeedback.vue'

const props = defineProps<{
  problemId: number
  version: number
  language: LanguageId
}>()
const {
  data: problem,
  error,
  loading,
  reload,
} = useQuery(
  () => [props.problemId, props.version] as const,
  ([id, version], signal) => getProblemVersion(id, version, { signal }),
)
const limits = computed(() =>
  problem.value?.languages.find((item) => item.language_id === props.language),
)
</script>

<template>
  <section class="surface fixed-problem" aria-label="判题版本题面">
    <details>
      <summary>
        判题版本题面 · 题目 #{{ problemId }} / v{{ version }}
        <span v-if="error"> · 题面未加载</span>
      </summary>
      <div class="fixed-problem-content">
        <RequestFeedback
          :loading="loading"
          loading-text="正在加载判题版本题面…"
          :error="error"
          @retry="reload"
        />
        <p v-if="error" class="result-note">
          题面信息未加载，以上判题结果仍可查看。
        </p>
        <template v-if="problem">
          <h2>{{ problem.title }}</h2>
          <p v-if="limits" class="result-note">
            {{ language === 'c' ? 'C' : 'Python 3' }} · 单用例限额：CPU
            {{ limits.cpu_time_ms }} ms，墙钟 {{ limits.wall_time_ms }} ms，内存
            {{ limits.memory_kib / 1024 }} MiB。
          </p>
          <ProblemStatement :statement="problem.statement" />
        </template>
      </div>
    </details>
  </section>
</template>
