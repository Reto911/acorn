<script setup lang="ts">
import { defineAsyncComponent } from 'vue'
import { ElButton } from 'element-plus'
import { RouterLink, useRoute } from 'vue-router'
import { getProblem } from '../api'
import PendingSubmissionNotice from '../components/PendingSubmissionNotice.vue'
import ProblemStatement from '../components/ProblemStatement.vue'
import RequestFeedback from '../components/RequestFeedback.vue'
import { useQuery } from '../composables/useQuery'
import 'element-plus/es/components/button/style/css'

const route = useRoute()
const {
  data: problem,
  loading,
  error,
  reload,
} = useQuery(
  () => route.params.problemId,
  (id, signal) => getProblem(Number(id), { signal }),
)
const CodeEditor = defineAsyncComponent(
  () => import('../components/CodeEditor.vue'),
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
      <section class="surface editor-panel" aria-labelledby="editor-title">
        <div class="panel-heading">
          <h2 id="editor-title">代码编辑区</h2>
          <span class="muted">暂未开放编辑与提交</span>
        </div>
        <div class="language-limits">
          <h3>允许语言与单用例限额</h3>
          <div
            v-for="language in problem.languages"
            :key="language.language_id"
            class="language-limit"
          >
            <h4>{{ language.language_id === 'c' ? 'C' : 'Python 3' }}</h4>
            <dl>
              <div>
                <dt>CPU 时间</dt>
                <dd>{{ language.cpu_time_ms }} ms</dd>
              </div>
              <div>
                <dt>墙钟时间</dt>
                <dd>{{ language.wall_time_ms }} ms</dd>
              </div>
              <div>
                <dt>内存</dt>
                <dd>{{ language.memory_kib / 1024 }} MiB</dd>
              </div>
            </dl>
          </div>
          <p>提交时将使用创建时的最新发布版本。</p>
        </div>
        <CodeEditor />
      </section>
    </div>
  </section>
</template>
