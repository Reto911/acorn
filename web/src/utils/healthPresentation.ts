import type {
  HealthObservation,
  HealthProbeErrorCode,
  HealthSnapshot,
  WorkerStatus,
} from '../api/types'
import { submissionDateFormat as dateFormat } from './submissionPresentation'

/** Labels describe the observed dimension without deriving overall availability. */
export const observationLabels: Record<HealthObservation['state'], string> = {
  unknown: '尚未取得结果',
  ok: '检查成功',
  failed: '检查失败',
  stale: '观测已过期',
}

export const admissionLabels: Record<
  HealthSnapshot['submission_admission'],
  string
> = {
  open: '开放新提交接纳',
  full: '待接纳名额已满',
  stopping: '正在停止接纳',
  unavailable: '调度已结束，无法接纳新提交',
}

export const schedulerLabels: Record<
  HealthSnapshot['scheduler']['state'],
  string
> = {
  running: '运行中',
  stopping: '停止中',
  stopped: '已停止',
}

export const executionLabels: Record<WorkerStatus['execution_state'], string> =
  {
    idle: '空闲',
    running: '执行中',
    blocked: '执行位置阻塞',
  }

export const probeErrorLabels: Record<HealthProbeErrorCode, string> = {
  ProbeTimeout: '检查超时，未在预算内取得结果',
  DatabaseReadFailed: '数据库读取检查失败',
  WorkerRpcFailed: 'Worker 通信或服务检查失败',
  WorkerStatusUnsupported: 'Worker 不支持状态查询接口',
  InvalidWorkerStatus: 'Worker 返回的状态不符合协议',
}

/** A missing check time means no result, never the Unix epoch. */
export function healthTime(value: number | null) {
  return value === null ? '尚未检查' : dateFormat.format(value)
}

/** Only the backend's stale projection adds an object to the warning. */
export function staleObservations(snapshot: HealthSnapshot) {
  return [
    { id: 'database', label: '数据库', ...snapshot.database },
    ...snapshot.workers.map((worker) => ({ ...worker, label: worker.id })),
  ].filter((observation) => observation.state === 'stale')
}
