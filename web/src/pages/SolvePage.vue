<script setup lang="ts">
import { ElButton } from 'element-plus'
import { ref, watch } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import { getProblem } from '../api'
import PendingSubmissionNotice from '../components/PendingSubmissionNotice.vue'
import ProblemStatement from '../components/ProblemStatement.vue'
import RequestFeedback from '../components/RequestFeedback.vue'
import SubmissionEditor from '../components/SubmissionEditor.vue'
import { useQuery } from '../composables/useQuery'
import { useAuth } from '../composables/useAuth'
import 'element-plus/es/components/button/style/css'

const route = useRoute()
const auth = useAuth()
const editorGeneration = ref(0)
watch(
  () => auth.user.value?.id ?? null,
  (owner, previousOwner) => {
    if (previousOwner !== null && owner !== previousOwner)
      editorGeneration.value += 1
  },
  { flush: 'sync' },
)
const {
  data: problem,
  loading,
  error,
  reload,
} = useQuery(
  () => route.params.problemId,
  (id, signal) => getProblem(Number(id), { signal }),
)
</script>

<template>
  <section aria-labelledby="page-title">
    <div class="page-heading">
      <RouterLink class="back-link" to="/problems">← 返回题目列表</RouterLink>
      <h1 id="page-title" class="problem-title">
        <template v-if="problem">
          #{{ problem.problem_id }} {{ problem.title }}
        </template>
        <template v-else>题目 #{{ route.params.problemId }}</template>
      </h1>
      <div v-if="problem" class="problem-actions">
        <span class="version-label"
          >当前展示版本 v{{ problem.version_no }}</span
        >
        <RouterLink
          class="text-link"
          :to="{
            path: '/submissions',
            query: { problem_id: problem.problem_id },
          }"
        >
          本题提交记录
        </RouterLink>
        <ElButton :loading="loading" :disabled="loading" @click="reload">
          刷新题面
        </ElButton>
      </div>
    </div>
    <PendingSubmissionNotice />
    <div v-if="loading || error" class="surface query-feedback solve-feedback">
      <RequestFeedback
        :loading="loading"
        loading-text="正在加载题面与语言限额…"
        :error="error"
        @retry="reload"
      />
      <p v-if="problem && error" class="stale-notice" role="status">
        以下为上次成功加载的题面与限额，内容可能已过期。
      </p>
    </div>
    <div v-if="problem" class="solve-layout">
      <section class="surface" aria-labelledby="statement-title">
        <div class="panel-heading"><h2 id="statement-title">题面</h2></div>
        <div class="statement-scroll">
          <ProblemStatement :statement="problem.statement" />
        </div>
      </section>
      <SubmissionEditor
        :key="`${problem.problem_id}:${editorGeneration}`"
        :problem="problem"
        :submission-disabled="loading || !!error"
      />
    </div>
  </section>
</template>
