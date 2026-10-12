import { readonly, ref, shallowRef } from 'vue'
import { createSubmission } from '../api'
import { endsCreation } from '../api/feedback'
import { RequestError } from '../api/http'
import type { CreateSubmissionRequest } from '../api/types'
import { exceptionContext, log } from '../utils/log'

export const PENDING_SUBMISSION_KEY = 'acorn:pending-submission:v1'

export interface PendingSubmission {
  readonly request_key: string
  readonly body: Readonly<CreateSubmissionRequest>
  readonly submission_id?: string
}

type SessionStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

function freezeRequest(
  requestKey: string,
  body: CreateSubmissionRequest,
  submissionId?: string,
): PendingSubmission {
  return Object.freeze({
    request_key: requestKey,
    body: Object.freeze({
      problem_id: body.problem_id,
      language_id: body.language_id,
      source: body.source,
    }),
    ...(submissionId ? { submission_id: submissionId } : {}),
  })
}

/** Validate storage data without logging the saved source or idempotency key. */
function restoreRequest(text: string): PendingSubmission {
  const value: unknown = JSON.parse(text)
  if (
    typeof value !== 'object' ||
    value === null ||
    !('request_key' in value) ||
    typeof value.request_key !== 'string' ||
    !/^[\da-f]{8}-[\da-f]{4}-[\da-f]{4}-[\da-f]{4}-[\da-f]{12}$/i.test(
      value.request_key,
    ) ||
    !('body' in value) ||
    typeof value.body !== 'object' ||
    value.body === null
  )
    throw new Error('Invalid saved submission request')
  const body = value.body
  if (
    !('problem_id' in body) ||
    typeof body.problem_id !== 'number' ||
    !Number.isSafeInteger(body.problem_id) ||
    body.problem_id <= 0 ||
    !('language_id' in body) ||
    (body.language_id !== 'c' && body.language_id !== 'python') ||
    !('source' in body) ||
    typeof body.source !== 'string' ||
    body.source.length === 0 ||
    ('submission_id' in value && typeof value.submission_id !== 'string')
  )
    throw new Error('Invalid saved submission content')
  return freezeRequest(
    value.request_key,
    {
      problem_id: body.problem_id,
      language_id: body.language_id,
      source: body.source,
    },
    'submission_id' in value ? (value.submission_id as string) : undefined,
  )
}

/** One tab owns one attempt. Drafts are separate and never read or modified here. */
export function createSubmissionRequest(storage: SessionStorage | null) {
  const pending = shallowRef<PendingSubmission | null>(null)
  const sending = ref(false)
  const error = shallowRef<RequestError | null>(null)
  const restored = ref(false)
  const storageWarning = ref<string | null>(null)

  function storageFailure(operation: string, message: string, cause?: unknown) {
    storageWarning.value = message
    log('warn', { operation, ...exceptionContext(cause) })
  }

  if (storage) {
    try {
      const text = storage.getItem(PENDING_SUBMISSION_KEY)
      if (text !== null) {
        pending.value = restoreRequest(text)
        restored.value = true
      }
    } catch (cause) {
      storageFailure(
        'restore-submission-request',
        '无法恢复上次提交请求，请先查看提交历史；不能用当前草稿重试上次请求。',
        cause,
      )
    }
  } else {
    storageFailure(
      'open-session-storage',
      '本标签页的会话存储不可用，原请求仅保留在内存中，刷新或关闭后无法恢复重试。',
    )
  }

  function persist() {
    if (!storage) return
    try {
      if (pending.value)
        storage.setItem(PENDING_SUBMISSION_KEY, JSON.stringify(pending.value))
      else storage.removeItem(PENDING_SUBMISSION_KEY)
      storageWarning.value = null
    } catch (cause) {
      storageFailure(
        'save-submission-request',
        pending.value
          ? '原请求仅保留在内存中，无法保存到会话存储；刷新后可能无法恢复本次请求。'
          : '无法清除会话存储中的原请求，刷新后可能再次出现；请先查看提交历史。',
        cause,
      )
    }
  }

  function clear() {
    pending.value = null
    restored.value = false
    persist()
  }

  async function retry() {
    if (sending.value || !pending.value) return null
    const snapshot = pending.value
    sending.value = true
    error.value = null
    try {
      const result = await createSubmission(snapshot.request_key, snapshot.body)
      clear()
      return result
    } catch (cause) {
      // The API layer converts HTTP, parsing, timeout and network failures.
      if (!(cause instanceof RequestError)) throw cause
      error.value = cause
      if (endsCreation(cause)) clear()
      else if (cause.submissionId) {
        pending.value = freezeRequest(
          snapshot.request_key,
          snapshot.body,
          cause.submissionId,
        )
        persist()
      }
      return null
    } finally {
      sending.value = false
    }
  }

  async function start(body: CreateSubmissionRequest) {
    if (sending.value || pending.value) return null
    pending.value = freezeRequest(crypto.randomUUID(), body)
    restored.value = false
    persist()
    return retry()
  }

  /** Call only after UI confirmation; abandoning cannot cancel a saved submission. */
  function abandonConfirmed() {
    if (sending.value) return false
    clear()
    error.value = null
    return true
  }

  return {
    pending: readonly(pending),
    sending: readonly(sending),
    error: readonly(error),
    restored: readonly(restored),
    storageWarning: readonly(storageWarning),
    start,
    retry,
    abandonConfirmed,
  }
}

let tabRequest: ReturnType<typeof createSubmissionRequest> | undefined

export function useSubmissionRequest() {
  if (!tabRequest) {
    let storage: SessionStorage | null = null
    try {
      storage = window.sessionStorage
    } catch {
      // Access itself may be denied; the controller shows the storage warning.
    }
    tabRequest = createSubmissionRequest(storage)
  }
  return tabRequest
}
