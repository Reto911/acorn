/** Public HTTP JSON contracts; source and Worker progress are never query fields. */
export type LanguageId = 'c' | 'python'
export type SubmissionState =
  'queued' | 'dispatching' | 'running' | 'finished' | 'failed'
export type Verdict = 'AC' | 'WA' | 'CE' | 'RE' | 'TLE' | 'MLE' | 'OLE'

export interface Page<T> {
  items: T[]
  limit: number
  offset: number
}

export interface PageQuery {
  limit?: number
  offset?: number
}

export interface ProblemSummary {
  problem_id: number
  version_no: number
  title: string
}

export interface ProblemLanguage {
  language_id: LanguageId
  cpu_time_ms: number
  wall_time_ms: number
  memory_kib: number
}

export interface Problem extends ProblemSummary {
  statement: string
  languages: ProblemLanguage[]
}

export interface CreateSubmissionRequest {
  problem_id: number
  language_id: LanguageId
  source: string
}

export interface CreateSubmissionResponse {
  submission_id: string
  state: SubmissionState
}

export interface SubmissionSummary {
  submission_id: string
  problem_id: number
  version_no: number
  language_id: LanguageId
  state: SubmissionState
  verdict: Verdict | null
  created_at_ms: number
  terminal_at_ms: number | null
}

/** Measurements use microseconds and bytes; absence differs from explicit zero. */
export interface ResourceUsage {
  cpu_time_us?: number
  wall_time_us?: number
  peak_memory_bytes?: number
}

export interface CompileDetails {
  compile_diagnostic_base64: string
  compile_diagnostic_truncated: boolean
}

export interface FailedCase {
  case_index: number
  stdout_base64: string
  stderr_base64: string
  stdout_truncated: boolean
  stderr_truncated: boolean
  resources?: ResourceUsage
  exit_code?: number
  signal?: number
}

export interface CaseDetails {
  failed_case: FailedCase
}

export interface ExecutionFailure {
  code: string
  diagnostic: string
}

/** State and verdict discriminate the terminal shapes without invented placeholders. */
export type Submission = SubmissionSummary &
  (
    | {
        state: 'queued' | 'dispatching' | 'running'
        verdict: null
        terminal_details: null
        terminal_at_ms: null
      }
    | { state: 'finished'; verdict: 'AC'; terminal_details: null }
    | { state: 'finished'; verdict: 'CE'; terminal_details: CompileDetails }
    | {
        state: 'finished'
        verdict: Exclude<Verdict, 'AC' | 'CE'>
        terminal_details: CaseDetails
      }
    | { state: 'failed'; verdict: null; terminal_details: ExecutionFailure }
  )

export interface HistoryQuery extends PageQuery {
  problem_id?: number
  state?: SubmissionState
  verdict?: Verdict
}

export interface HttpErrorBody {
  code: string
  message: string
  submission_id?: string
}
