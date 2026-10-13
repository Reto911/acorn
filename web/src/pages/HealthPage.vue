<script setup lang="ts">
import { computed } from 'vue'
import { ElAlert, ElButton, ElTable, ElTableColumn } from 'element-plus'
import { getHealth } from '../api'
import RequestFeedback from '../components/RequestFeedback.vue'
import { useQuery } from '../composables/useQuery'
import {
  admissionLabels,
  executionLabels,
  healthTime,
  observationLabels,
  probeErrorLabels,
  schedulerLabels,
  staleObservations,
} from '../utils/healthPresentation'
import 'element-plus/es/components/alert/style/css'
import 'element-plus/es/components/button/style/css'
import 'element-plus/es/components/table/style/css'
import 'element-plus/es/components/table-column/style/css'

const {
  data: snapshot,
  loading,
  error,
  reload,
} = useQuery(
  () => 'health',
  (_query, signal) => getHealth({ signal }),
)
const stale = computed(() =>
  snapshot.value ? staleObservations(snapshot.value) : [],
)
const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone
</script>

<template>
  <section aria-labelledby="page-title">
    <div class="page-heading">
      <h1 id="page-title">系统状态</h1>
      <p>
        查看获取时快照。各项指标仅反映对应观测，不保证下一次提交或判题成功。
      </p>
    </div>
    <div class="surface health-toolbar">
      <div class="panel-heading">
        <div>
          <h2>获取时快照</h2>
          <p class="muted">
            {{
              snapshot
                ? `快照时间：${healthTime(snapshot.snapshot_at_ms)}`
                : '尚未取得快照'
            }}
            · {{ timezone }}
          </p>
        </div>
        <ElButton :loading="loading" :disabled="loading" @click="reload">
          刷新状态
        </ElButton>
      </div>
      <div class="query-feedback">
        <RequestFeedback
          :loading="loading"
          loading-text="正在获取系统状态…"
          :error="error"
          @retry="reload"
        />
        <p v-if="error && snapshot" class="stale-notice" role="status">
          以下为上次成功获取的旧快照，内容可能已过期；请重试获取最新快照。
        </p>
      </div>
      <p class="health-note">
        页面仅在进入或手动刷新时获取快照；刷新读取后端缓存，各项检查时间可能不同。
      </p>
    </div>

    <template v-if="snapshot">
      <ElAlert
        v-if="stale.length"
        class="health-stale-warning"
        title="以下观测信息已过期，请刷新后查看检查结果"
        type="warning"
        :closable="false"
        show-icon
      >
        <ul>
          <li v-for="observation in stale" :key="observation.id">
            {{ observation.label }} · 最后检查时间：{{
              healthTime(observation.checked_at_ms)
            }}
          </li>
        </ul>
      </ElAlert>

      <div class="health-summary">
        <section class="surface" aria-labelledby="backend-title">
          <div class="panel-heading">
            <h2 id="backend-title">后端本地状态</h2>
          </div>
          <dl class="health-facts">
            <div>
              <dt>新提交接纳</dt>
              <dd>{{ admissionLabels[snapshot.submission_admission] }}</dd>
            </div>
            <div>
              <dt>Scheduler 生命周期</dt>
              <dd>{{ schedulerLabels[snapshot.scheduler.state] }}</dd>
            </div>
            <div>
              <dt>待接纳名额占用 / 容量</dt>
              <dd>
                {{ snapshot.scheduler.pending_used }} /
                {{ snapshot.scheduler.pending_capacity }}
              </dd>
            </div>
          </dl>
          <p class="health-note">
            名额占用包含创建预留、排队和等待接纳确认的任务；实际提交以创建接口结果为准。
          </p>
        </section>
        <section class="surface" aria-labelledby="database-title">
          <div class="panel-heading"><h2 id="database-title">数据库</h2></div>
          <dl class="health-facts">
            <div>
              <dt>读取检查</dt>
              <dd>
                {{
                  snapshot.database.state === 'ok'
                    ? '读取正常'
                    : observationLabels[snapshot.database.state]
                }}
              </dd>
            </div>
            <div>
              <dt>最后检查时间</dt>
              <dd>{{ healthTime(snapshot.database.checked_at_ms) }}</dd>
            </div>
            <div v-if="snapshot.database.error_code">
              <dt>检查错误</dt>
              <dd>
                {{ probeErrorLabels[snapshot.database.error_code]
                }}<br /><code>{{ snapshot.database.error_code }}</code>
              </dd>
            </div>
          </dl>
          <p class="health-note">
            读取正常表示该次检查完成，不保证后续写入成功。
          </p>
        </section>
      </div>

      <section class="surface" aria-labelledby="workers-title">
        <div class="panel-heading"><h2 id="workers-title">Worker 观测</h2></div>
        <ElTable
          :data="snapshot.workers"
          class="health-workers"
          empty-text="未配置 Worker"
        >
          <ElTableColumn prop="id" label="节点" width="100" />
          <ElTableColumn label="检查结果" min-width="190">
            <template #default="{ row }">
              <p>
                {{
                  observationLabels[row.state as keyof typeof observationLabels]
                }}
              </p>
              <p v-if="row.error_code" class="health-probe-error">
                {{
                  probeErrorLabels[
                    row.error_code as keyof typeof probeErrorLabels
                  ]
                }}<br />
                <code>{{ row.error_code }}</code>
              </p>
            </template>
          </ElTableColumn>
          <ElTableColumn label="执行状态" min-width="110">
            <template #default="{ row }">{{
              row.state === 'ok' && row.status
                ? executionLabels[
                    row.status.execution_state as keyof typeof executionLabels
                  ]
                : '未取得有效状态'
            }}</template>
          </ElTableColumn>
          <ElTableColumn label="停机标记" min-width="110">
            <template #default="{ row }">{{
              row.state === 'ok' && row.status
                ? row.status.stopping
                  ? '停机中'
                  : '未停机'
                : '未取得有效状态'
            }}</template>
          </ElTableColumn>
          <ElTableColumn label="未确认结果 / 容量" min-width="145">
            <template #default="{ row }">{{
              row.state === 'ok' && row.status
                ? `${row.status.unacknowledged_results} / ${row.status.unacknowledged_capacity}`
                : '未取得有效状态'
            }}</template>
          </ElTableColumn>
          <ElTableColumn label="最后检查时间" min-width="180">
            <template #default="{ row }">{{
              healthTime(row.checked_at_ms)
            }}</template>
          </ElTableColumn>
        </ElTable>
        <p class="health-note">
          节点标识按配置顺序生成。检查失败表示未取得有效状态，不能据此断言
          Worker 已停止；执行状态、停机标记与结果数量分别展示。
        </p>
      </section>
    </template>
  </section>
</template>

<style scoped>
.health-toolbar,
.health-stale-warning,
.health-summary {
  margin-bottom: 24px;
}

.health-toolbar .muted {
  margin-top: 8px;
  line-height: 1.8;
}

.health-summary {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 24px;
}

.health-facts {
  display: grid;
  gap: 16px;
  margin: 0;
  padding: 20px;
}

.health-facts dt,
.health-note {
  color: #68756c;
  font-size: 13px;
  line-height: 1.8;
}

.health-facts dd {
  margin: 4px 0 0;
  overflow-wrap: anywhere;
}

.health-note {
  padding: 0 20px 20px;
}

.health-stale-warning ul {
  margin: 8px 0 0;
  padding-left: 20px;
  line-height: 1.8;
}

.health-probe-error {
  margin-top: 8px;
  font-size: 12px;
  overflow-wrap: anywhere;
}

.health-workers + .health-note {
  padding-top: 16px;
}

@media (max-width: 1023px) {
  .health-summary {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
