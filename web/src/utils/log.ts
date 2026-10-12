import type { SubmissionState } from '../api/types'

/** Only explicit diagnostic metadata is accepted, never payloads or request headers. */
export interface LogContext {
  operation: string
  path?: string
  method?: string
  status?: number
  code?: string
  submission_id?: string
  duration_ms?: number
  state?: SubmissionState
  previous_state?: SubmissionState
  reason?: string
  stack?: string
}

export function log(
  level: 'debug' | 'info' | 'warn' | 'error',
  context: LogContext,
) {
  if (level === 'debug' && !import.meta.env.DEV) return
  console[level]('[Acorn]', { time: new Date().toISOString(), ...context })
}

/** Keep stack frames while omitting exception messages that may contain user text. */
export function exceptionContext(reason: unknown) {
  return reason instanceof Error
    ? {
        reason: reason.name,
        stack: reason.stack
          ?.split('\n')
          .filter((line) => /^\s*(at |[^@]*@(?:https?:|file:))/.test(line))
          .join('\n'),
      }
    : { reason: typeof reason }
}

export function logSubmissionState(
  submissionId: string,
  state: SubmissionState,
  previousState?: SubmissionState,
) {
  if (state === previousState) return
  log('info', {
    operation: 'submission-state',
    submission_id: submissionId,
    state,
    previous_state: previousState,
  })
}
