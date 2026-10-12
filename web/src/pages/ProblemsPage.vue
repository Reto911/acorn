<script setup lang="ts">
import { ElButton, ElTable, ElTableColumn } from 'element-plus'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { listProblems } from '../api'
import RequestFeedback from '../components/RequestFeedback.vue'
import { useQuery } from '../composables/useQuery'
import 'element-plus/es/components/button/style/css'
import 'element-plus/es/components/table/style/css'
import 'element-plus/es/components/table-column/style/css'

const route = useRoute()
const router = useRouter()
const {
  data: page,
  loading,
  error,
  reload,
} = useQuery(
  () => ({ limit: route.query.limit, offset: route.query.offset }),
  (query, signal) =>
    listProblems(
      {
        // Omit defaults on first entry; invalid URL numbers are rejected by the API.
        limit: query.limit === undefined ? undefined : Number(query.limit),
        offset: query.offset === undefined ? undefined : Number(query.offset),
      },
      { signal },
    ),
)

function changePage(direction: -1 | 1) {
  if (!page.value) return
  const { limit, offset } = page.value
  void router.push({
    path: '/problems',
    query: { limit, offset: Math.max(0, offset + direction * limit) },
  })
}
</script>

<template>
  <section aria-labelledby="page-title">
    <div class="page-heading">
      <h1 id="page-title">题目</h1>
      <p>从一道题目开始，编写代码并查看判题结果。</p>
    </div>
    <div class="surface problem-list">
      <div class="panel-heading">
        <h2>已发布题目</h2>
        <ElButton :loading="loading" :disabled="loading" @click="reload">
          刷新列表
        </ElButton>
      </div>
      <div class="query-feedback">
        <RequestFeedback
          :loading="loading"
          loading-text="正在加载题目列表…"
          :error="error"
          :empty="page?.items.length === 0"
          :empty-text="page?.offset === 0 ? '暂无已发布题目' : '本页暂无题目'"
          :empty-description="
            page?.offset === 0
              ? '题目发布后会显示在这里。'
              : '已到达列表末尾，可返回上一页。'
          "
          @retry="reload"
        />
        <p v-if="error && page" class="stale-notice" role="status">
          以下为上次成功加载的列表，内容可能已过期。
        </p>
        <RouterLink v-if="error" class="text-link" to="/problems">
          返回列表首页
        </RouterLink>
      </div>
      <ElTable v-if="page?.items.length" :data="page.items">
        <ElTableColumn prop="problem_id" label="题号" width="76" />
        <ElTableColumn label="标题" min-width="120">
          <template #default="{ row }">
            <RouterLink
              class="text-link problem-title"
              :to="`/problems/${row.problem_id}`"
            >
              {{ row.title }}
            </RouterLink>
          </template>
        </ElTableColumn>
        <ElTableColumn label="最新版本" width="96">
          <template #default="{ row }">v{{ row.version_no }}</template>
        </ElTableColumn>
      </ElTable>
      <nav v-if="page" class="list-pagination" aria-label="题目列表分页">
        <span class="muted">
          每页 {{ page.limit }} 题 · 本页 {{ page.items.length }} 题
        </span>
        <div>
          <ElButton
            :disabled="loading || !!error || page.offset === 0"
            @click="changePage(-1)"
          >
            上一页
          </ElButton>
          <ElButton
            :disabled="loading || !!error || page.items.length < page.limit"
            @click="changePage(1)"
          >
            下一页
          </ElButton>
        </div>
      </nav>
    </div>
  </section>
</template>
