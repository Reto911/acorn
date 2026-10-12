import type { SubmissionState, Verdict } from '../api/types'

/** Shared labels keep history filters, summaries and terminal results consistent. */
export const submissionStates: Record<SubmissionState, string> = {
  queued: '排队中',
  dispatching: '分派中',
  running: '判题中',
  finished: '已完成',
  failed: '执行失败',
}

export const submissionVerdicts: Record<Verdict, string> = {
  AC: '通过',
  WA: '答案错误',
  CE: '编译错误',
  RE: '运行错误',
  TLE: '时间超限',
  MLE: '内存超限',
  OLE: '输出超限',
}

/** Result and history timestamps use the same Chinese format and local timezone. */
export const submissionDateFormat = new Intl.DateTimeFormat('zh-CN', {
  dateStyle: 'medium',
  timeStyle: 'medium',
})
