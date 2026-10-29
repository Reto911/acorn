import { log } from '../utils/log'
import { requestJson } from './http'
import type {
  AuthResponse,
  LoginRequest,
  RegisterRequest,
  CreateSubmissionRequest,
  CreateSubmissionResponse,
  HistoryQuery,
  HealthSnapshot,
  Page,
  PageQuery,
  Problem,
  ProblemSummary,
  Submission,
  SubmissionSummary,
  ReviewResponse,
} from './types'

export interface QueryOptions {
  signal?: AbortSignal
  expectedUserId?: number
}

/** Authentication responses contain only a nullable public identity. */
function validAuth(value: unknown, nullable: boolean) {
  if (typeof value !== 'object' || value === null || !('user' in value))
    return false
  if (value.user === null) return nullable
  if (typeof value.user !== 'object') return false
  const user = value.user
  return (
    'id' in user &&
    typeof user.id === 'number' &&
    Number.isSafeInteger(user.id) &&
    user.id > 0 &&
    'username' in user &&
    typeof user.username === 'string' &&
    'display_name' in user &&
    typeof user.display_name === 'string'
  )
}

export async function getMe(options: QueryOptions = {}) {
  return (
    await requestJson<AuthResponse>('/api/v1/auth/me', {
      operation: 'get-me',
      signal: options.signal,
      validate: (value, status) => status === 200 && validAuth(value, true),
    })
  ).data
}

export async function login(request: LoginRequest, options: QueryOptions = {}) {
  return (
    await requestJson<AuthResponse>('/api/v1/auth/login', {
      operation: 'login',
      method: 'POST',
      signal: options.signal,
      body: { username: request.username, password: request.password },
      validate: (value, status) => status === 200 && validAuth(value, false),
    })
  ).data
}

export async function register(
  request: RegisterRequest,
  options: QueryOptions = {},
) {
  return (
    await requestJson<AuthResponse>('/api/v1/auth/register', {
      operation: 'register',
      method: 'POST',
      signal: options.signal,
      body: {
        username: request.username,
        password: request.password,
        ...(request.display_name === undefined
          ? {}
          : { display_name: request.display_name }),
      },
      validate: (value, status) => status === 201 && validAuth(value, false),
    })
  ).data
}

export async function logout(options: QueryOptions = {}) {
  return (
    await requestJson<AuthResponse>('/api/v1/auth/logout', {
      operation: 'logout',
      method: 'POST',
      ...options,
      body: {},
      validate: (value, status) =>
        status === 200 &&
        validAuth(value, true) &&
        (value as AuthResponse).user === null,
    })
  ).data
}

/** Read the backend snapshot without triggering component probes. */
export async function getHealth(options: QueryOptions = {}) {
  return (
    await requestJson<HealthSnapshot>('/api/v1/health', {
      operation: 'get-health',
      signal: options.signal,
    })
  ).data
}

/** Pick parameters explicitly so route queries cannot introduce unallowed fields. */
function queryString(query: HistoryQuery) {
  const params = new URLSearchParams()
  for (const key of [
    'limit',
    'offset',
    'problem_id',
    'state',
    'verdict',
    'review_status',
  ] as const) {
    if (query[key] !== undefined) params.set(key, String(query[key]))
  }
  const result = params.toString()
  return result ? `?${result}` : ''
}

export async function listProblems(
  query: PageQuery = {},
  options: QueryOptions = {},
) {
  return (
    await requestJson<Page<ProblemSummary>>(
      `/api/v1/problems${queryString({ limit: query.limit, offset: query.offset })}`,
      { operation: 'list-problems', ...options },
    )
  ).data
}

export async function getProblem(
  problemId: number,
  options: QueryOptions = {},
) {
  return (
    await requestJson<Problem>(`/api/v1/problems/${problemId}`, {
      operation: 'get-problem',
      ...options,
    })
  ).data
}

export async function getProblemVersion(
  problemId: number,
  version: number,
  options: QueryOptions = {},
) {
  return (
    await requestJson<Problem>(
      `/api/v1/problems/${problemId}/versions/${version}`,
      { operation: 'get-problem-version', ...options },
    )
  ).data
}

export async function listSubmissions(
  query: HistoryQuery = {},
  options: QueryOptions = {},
) {
  return (
    await requestJson<Page<SubmissionSummary>>(
      `/api/v1/submissions${queryString(query)}`,
      { operation: 'list-submissions', ...options },
    )
  ).data
}

export async function getSubmission(
  submissionId: string,
  options: QueryOptions & { polling?: boolean } = {},
) {
  const path = `/api/v1/submissions/${encodeURIComponent(submissionId)}`
  const result = await requestJson<Submission>(path, {
    operation: 'get-submission',
    signal: options.signal,
    expectedUserId: options.expectedUserId,
  })
  if (options.polling)
    log('debug', {
      operation: 'poll-submission',
      path,
      method: 'GET',
      status: result.status,
      submission_id: submissionId,
      duration_ms: result.duration_ms,
    })
  return result.data
}

/** Send one explicit review request using the caller's latest persisted identity; never retry. */
export async function startReview(
  submissionId: string,
  expectedReviewId: string | null,
  options: QueryOptions = {},
) {
  const path = `/api/v1/submissions/${encodeURIComponent(submissionId)}/review`
  const result = await requestJson<ReviewResponse>(path, {
    operation: 'start-review',
    method: 'POST',
    signal: options.signal,
    expectedUserId: options.expectedUserId,
    body: { expected_review_id: expectedReviewId },
    validate: (value, status) => {
      if (
        (status !== 200 && status !== 202) ||
        typeof value !== 'object' ||
        value === null
      )
        return false
      const response = value as Partial<ReviewResponse>
      return (
        response.submission_id === submissionId &&
        ['PENDING', 'PASS', 'DOUBTFUL', 'FAILED'].includes(
          response.review_status ?? '',
        ) &&
        typeof response.review === 'object' &&
        response.review !== null &&
        typeof response.review.review_id === 'string' &&
        response.review.review_id.length > 0
      )
    },
  })
  log('info', {
    operation: 'start-review',
    path,
    method: 'POST',
    status: result.status,
    submission_id: submissionId,
    duration_ms: result.duration_ms,
  })
  return result.data
}

function validCreation(value: unknown, status: number) {
  return (
    (status === 200 || status === 202) &&
    typeof value === 'object' &&
    value !== null &&
    'submission_id' in value &&
    typeof value.submission_id === 'string' &&
    value.submission_id.length > 0 &&
    'state' in value &&
    typeof value.state === 'string' &&
    ['queued', 'dispatching', 'running', 'finished', 'failed'].includes(
      value.state,
    )
  )
}

/** Callers own the UUID and frozen content; this function never generates keys. */
export async function createSubmission(
  requestKey: string,
  snapshot: Readonly<CreateSubmissionRequest>,
  options: QueryOptions = {},
) {
  const result = await requestJson<CreateSubmissionResponse>(
    '/api/v1/submissions',
    {
      operation: 'create-submission',
      method: 'POST',
      signal: options.signal,
      expectedUserId: options.expectedUserId,
      idempotencyKey: requestKey,
      body: {
        problem_id: snapshot.problem_id,
        language_id: snapshot.language_id,
        source: snapshot.source,
      },
      validate: validCreation,
    },
  )
  log('info', {
    operation: 'create-submission',
    path: '/api/v1/submissions',
    method: 'POST',
    status: result.status,
    submission_id: result.data.submission_id,
    state: result.data.state,
    duration_ms: result.duration_ms,
  })
  return result.data
}
