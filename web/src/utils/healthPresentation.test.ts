import { describe, expect, it } from 'vitest'
import type { HealthSnapshot } from '../api/types'
import {
  admissionLabels,
  executionLabels,
  healthTime,
  observationLabels,
  probeErrorLabels,
  schedulerLabels,
  staleObservations,
} from './healthPresentation'

describe('health observation presentation', () => {
  it('keeps capacity, lifecycle, execution and check failures distinct', () => {
    expect(observationLabels).toEqual({
      unknown: '尚未取得结果',
      ok: '检查成功',
      failed: '检查失败',
      stale: '观测已过期',
    })
    expect(admissionLabels).toEqual({
      open: '开放新提交接纳',
      full: '待接纳名额已满',
      stopping: '正在停止接纳',
      unavailable: '调度已结束，无法接纳新提交',
    })
    expect(schedulerLabels).toEqual({
      running: '运行中',
      stopping: '停止中',
      stopped: '已停止',
    })
    expect(executionLabels).toEqual({
      idle: '空闲',
      running: '执行中',
      blocked: '执行位置阻塞',
    })
    expect(probeErrorLabels).toEqual({
      ProbeTimeout: '检查超时，未在预算内取得结果',
      DatabaseReadFailed: '数据库读取检查失败',
      WorkerRpcFailed: 'Worker 通信或服务检查失败',
      WorkerStatusUnsupported: 'Worker 不支持状态查询接口',
      InvalidWorkerStatus: 'Worker 返回的状态不符合协议',
    })
    expect(healthTime(null)).toBe('尚未检查')
    expect(healthTime(0)).not.toBe('尚未检查')
  })

  it('lists every stale object and its own check time, then removes only recovered objects', () => {
    const snapshot: HealthSnapshot = {
      snapshot_at_ms: 10_000,
      submission_admission: 'open',
      scheduler: { state: 'running', pending_used: 0, pending_capacity: 128 },
      database: { state: 'stale', checked_at_ms: 1000, error_code: null },
      workers: [
        {
          id: 'worker-1',
          state: 'stale',
          checked_at_ms: 2000,
          error_code: null,
          status: null,
        },
        {
          id: 'worker-2',
          state: 'failed',
          checked_at_ms: 3000,
          error_code: 'WorkerRpcFailed',
          status: null,
        },
        {
          id: 'worker-3',
          state: 'unknown',
          checked_at_ms: null,
          error_code: null,
          status: null,
        },
        {
          id: 'worker-4',
          state: 'stale',
          checked_at_ms: 4000,
          error_code: null,
          status: null,
        },
      ],
    }
    expect(
      staleObservations(snapshot).map(({ label, checked_at_ms }) => ({
        label,
        checked_at_ms,
      })),
    ).toEqual([
      { label: '数据库', checked_at_ms: 1000 },
      { label: 'worker-1', checked_at_ms: 2000 },
      { label: 'worker-4', checked_at_ms: 4000 },
    ])
    snapshot.database = { state: 'ok', checked_at_ms: 5000, error_code: null }
    snapshot.workers[0] = {
      id: 'worker-1',
      state: 'ok',
      checked_at_ms: 6000,
      error_code: null,
      status: {
        stopping: false,
        execution_state: 'idle',
        unacknowledged_results: 0,
        unacknowledged_capacity: 128,
      },
    }
    expect(staleObservations(snapshot).map(({ label }) => label)).toEqual([
      'worker-4',
    ])
    snapshot.workers.pop()
    expect(staleObservations(snapshot)).toEqual([])
    // Old timestamps alone never create browser-side expiry.
    expect(snapshot.database.checked_at_ms).toBeLessThan(Date.now() - 12_000)
  })
})
