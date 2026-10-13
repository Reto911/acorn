<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import { ElButton, ElTable, ElTableColumn } from 'element-plus'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { listSubmissions } from '../api'
import RequestFeedback from '../components/RequestFeedback.vue'
import { useQuery } from '../composables/useQuery'
import { readHistoryQuery } from '../utils/historyQuery'
import { reviewStatuses } from '../utils/reviewPresentation'
import {
  submissionDateFormat as dateFormat,
  submissionStates as states,
  submissionVerdicts as verdicts,
} from '../utils/submissionPresentation'
import 'element-plus/es/components/button/style/css'
import 'element-plus/es/components/table/style/css'
import 'element-plus/es/components/table-column/style/css'

const route = useRoute()
const router = useRouter()
const query = computed(() => readHistoryQuery(route.query))
const filters = reactive({
  problem_id: '' as string | number,
  state: '',
  verdict: '',
  review_status: '',
})
watch(
  query,
  (value) => {
    filters.problem_id =
      value.problem_id === undefined ? '' : String(value.problem_id)
    filters.state = value.state ?? ''
    filters.verdict = value.verdict ?? ''
    filters.review_status = value.review_status ?? ''
  },
  { immediate: true },
)

const {
  data: page,
  loading,
  error,
  reload,
} = useQuery(
  () => query.value,
  (value, signal) => listSubmissions(value, { signal }),
)
const hasFilters = computed(
  () =>
    query.value.problem_id !== undefined ||
    query.value.state !== undefined ||
    query.value.verdict !== undefined ||
    query.value.review_status !== undefined,
)
const pageQuery = computed(() => ({
  ...query.value,
  limit: page.value?.limit ?? query.value.limit,
  offset: page.value?.offset ?? query.value.offset,
}))
const timeZone = dateFormat.resolvedOptions().timeZone

function applyFilters() {
  void router.push({
    path: '/submissions',
    query: {
      limit: pageQuery.value.limit,
      offset: 0,
      problem_id: filters.problem_id || undefined,
      state: filters.state || undefined,
      verdict: filters.verdict || undefined,
      review_status: filters.review_status || undefined,
    },
  })
}

function clearFilters() {
  filters.problem_id = ''
  filters.state = ''
  filters.verdict = ''
  filters.review_status = ''
  void router.push({
    path: '/submissions',
    query: { limit: pageQuery.value.limit, offset: 0 },
  })
}

function changePage(offset: number) {
  void router.push({
    path: '/submissions',
    query: { ...pageQuery.value, offset },
  })
}
</script>

<template>
  <section aria-labelledby="page-title">
    <div class="page-heading">
      <h1 id="page-title">提交历史</h1>
      <p>查看后端配置用户的提交记录，包括其他浏览器创建的提交。</p>
    </div>
    <div class="surface history-list">
      <form
        class="history-filters"
        aria-label="历史筛选"
        @submit.prevent="applyFilters"
      >
        <div class="history-filter">
          <label for="history-problem">题号</label>
          <input
            id="history-problem"
            v-model="filters.problem_id"
            type="number"
            min="1"
            step="1"
            placeholder="全部题目"
          />
        </div>
        <div class="history-filter">
          <label for="history-state">状态</label>
          <select id="history-state" v-model="filters.state">
            <option value="">全部状态</option>
            <option
              v-for="(label, value) in states"
              :key="value"
              :value="value"
            >
              {{ label }}（{{ value }}）
            </option>
          </select>
        </div>
        <div class="history-filter">
          <label for="history-verdict">判题结论</label>
          <select id="history-verdict" v-model="filters.verdict">
            <option value="">全部结论</option>
            <option
              v-for="(label, value) in verdicts"
              :key="value"
              :value="value"
            >
              {{ label }}（{{ value }}）
            </option>
          </select>
        </div>
        <div class="history-filter">
          <label for="history-review">审查状态</label>
          <select id="history-review" v-model="filters.review_status">
            <option value="">全部审查状态</option>
            <option
              v-for="(label, value) in reviewStatuses"
              :key="value"
              :value="value"
            >
              {{ label }}
            </option>
          </select>
        </div>
        <div class="history-filter-actions">
          <ElButton type="primary" native-type="submit" :disabled="loading"
            >查询</ElButton
          >
          <ElButton :disabled="loading" @click="clearFilters"
            >清除筛选</ElButton
          >
        </div>
      </form>
      <div class="panel-heading">
        <h2>提交记录</h2>
        <ElButton :loading="loading" :disabled="loading" @click="reload"
          >刷新列表</ElButton
        >
      </div>
      <div class="query-feedback">
        <RequestFeedback
          :loading="loading"
          loading-text="正在加载提交历史…"
          :error="error"
          :empty="page?.items.length === 0"
          :empty-text="
            page && page.offset > 0
              ? '本页暂无提交'
              : hasFilters
                ? '没有符合筛选条件的提交'
                : '暂无提交记录'
          "
          :empty-description="
            page && page.offset > 0
              ? '已到达列表末尾，可返回上一页或回到首页。'
              : hasFilters
                ? '请调整或清除筛选条件。多个筛选条件需同时满足。'
                : '提交代码后，可在这里重新打开判题结果。'
          "
          @retry="reload"
        />
        <p v-if="error && page" class="stale-notice" role="status">
          以下为上次成功加载的列表，内容可能已过期，请手动重试。
        </p>
        <RouterLink v-if="error" class="text-link" to="/submissions"
          >打开全部提交首页</RouterLink
        >
      </div>
      <ElTable
        v-if="page?.items.length"
        :data="page.items"
        row-key="submission_id"
      >
        <ElTableColumn label="提交 ID" min-width="215">
          <template #default="{ row }">
            <RouterLink
              class="text-link history-id"
              :to="{
                name: 'result',
                params: { submissionId: row.submission_id },
                query: { ...pageQuery },
              }"
              >{{ row.submission_id }}</RouterLink
            >
          </template>
        </ElTableColumn>
        <ElTableColumn prop="problem_id" label="题号" width="56" />
        <ElTableColumn label="版本" width="56">
          <template #default="{ row }">v{{ row.version_no }}</template>
        </ElTableColumn>
        <ElTableColumn label="语言" width="78">
          <template #default="{ row }">{{
            row.language_id === 'c' ? 'C' : 'Python 3'
          }}</template>
        </ElTableColumn>
        <ElTableColumn label="状态" width="82">
          <template #default="{ row }">{{
            states[row.state as keyof typeof states]
          }}</template>
        </ElTableColumn>
        <ElTableColumn label="判题结论" width="112">
          <template #default="{ row }">{{
            row.verdict === null
              ? '未产生'
              : `${verdicts[row.verdict as keyof typeof verdicts]}（${row.verdict}）`
          }}</template>
        </ElTableColumn>
        <ElTableColumn label="审查状态" width="128">
          <template #default="{ row }">{{
            reviewStatuses[row.review_status as keyof typeof reviewStatuses]
          }}</template>
        </ElTableColumn>
        <ElTableColumn label="时间" min-width="200">
          <template #default="{ row }">
            <div class="history-time">
              <span>创建</span>{{ dateFormat.format(row.created_at_ms) }}
            </div>
            <div class="history-time">
              <span>终态</span
              >{{
                row.terminal_at_ms === null
                  ? '尚未产生'
                  : dateFormat.format(row.terminal_at_ms)
              }}
            </div>
          </template>
        </ElTableColumn>
      </ElTable>
      <nav v-if="page" class="list-pagination" aria-label="提交历史分页">
        <span class="muted"
          >每页 {{ page.limit }} 条 · 本页 {{ page.items.length }} 条</span
        >
        <div>
          <ElButton
            :disabled="loading || !!error || page.offset === 0"
            @click="changePage(Math.max(0, page.offset - page.limit))"
            >上一页</ElButton
          >
          <ElButton
            :disabled="loading || !!error || page.items.length < page.limit"
            @click="changePage(page.offset + page.limit)"
            >下一页</ElButton
          >
          <ElButton
            :disabled="loading || page.offset === 0"
            @click="changePage(0)"
            >回到首页</ElButton
          >
        </div>
      </nav>
      <p class="history-note">
        按创建时间倒序，同时间按提交 ID
        倒序；列表不自动刷新。新提交可能使分页位置变化，可回到首页并刷新查看最新记录。时间按浏览器本地时区
        {{ timeZone }} 显示。
      </p>
    </div>
  </section>
</template>
