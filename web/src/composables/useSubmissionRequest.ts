import {
  effectScope,
  onScopeDispose,
  readonly,
  ref,
  shallowRef,
  watch,
} from 'vue'
import { createSubmission } from '../api'
import { endsCreation } from '../api/feedback'
import { RequestError } from '../api/http'
import type { CreateSubmissionRequest } from '../api/types'
import { exceptionContext, log } from '../utils/log'
import { useAuth, type Identity } from './useAuth'

export function pendingSubmissionKey(userId: number) {
  return `acorn:pending-submission:v2:${userId}`
}

export interface PendingSubmission {
  readonly owner_user_id: number
  readonly request_key: string
  readonly body: Readonly<CreateSubmissionRequest>
  readonly submission_id?: string
}

type SessionStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

function freezeRequest(
  ownerUserId: number,
  requestKey: string,
  body: CreateSubmissionRequest,
  submissionId?: string,
): PendingSubmission {
  return Object.freeze({
    owner_user_id: ownerUserId,
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
function restoreRequest(text: string, ownerUserId: number): PendingSubmission {
  const value: unknown = JSON.parse(text)
  if (
    typeof value !== 'object' ||
    value === null ||
    !('owner_user_id' in value) ||
    value.owner_user_id !== ownerUserId ||
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
    ownerUserId,
    value.request_key,
    {
      problem_id: body.problem_id,
      language_id: body.language_id,
      source: body.source,
    },
    'submission_id' in value ? (value.submission_id as string) : undefined,
  )
}

type SubmissionIdentity = Pick<
  ReturnType<typeof useAuth>,
  | 'user'
  | 'generation'
  | 'snapshot'
  | 'isCurrent'
  | 'requireLogin'
  | 'handleError'
>

/** One tab exposes only its current user's attempt; other owners retain their keys. */
export function createSubmissionRequest(
  storage: SessionStorage | null,
  identity: SubmissionIdentity,
) {
  const pending = shallowRef<PendingSubmission | null>(null)
  const sending = ref(false)
  const error = shallowRef<RequestError | null>(null)
  const restored = ref(false)
  const storageWarning = ref<string | null>(null)
  const memory = new Map<number, PendingSubmission>()
  let ownerId: number | null = null
  let operationGeneration = 0
  let controller: AbortController | undefined

  function storageFailure(operation: string, message: string, cause?: unknown) {
    storageWarning.value = message
    log('warn', { operation, ...exceptionContext(cause) })
  }

  function restore() {
    if (ownerId === null) return
    const saved = memory.get(ownerId)
    if (saved) {
      pending.value = saved
      restored.value = true
      return
    }
    if (storage) {
      try {
        const text = storage.getItem(pendingSubmissionKey(ownerId))
        if (text !== null) {
          pending.value = restoreRequest(text, ownerId)
          memory.set(ownerId, pending.value)
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
  }

  function persist() {
    if (ownerId === null) return
    if (pending.value) memory.set(ownerId, pending.value)
    else memory.delete(ownerId)
    if (!storage) return
    try {
      if (pending.value)
        storage.setItem(
          pendingSubmissionKey(ownerId),
          JSON.stringify(pending.value),
        )
      else storage.removeItem(pendingSubmissionKey(ownerId))
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

  watch(
    () => [identity.user.value?.id ?? null, identity.generation.value] as const,
    ([userId]) => {
      operationGeneration += 1
      controller?.abort()
      controller = undefined
      sending.value = false
      pending.value = null
      error.value = null
      restored.value = false
      storageWarning.value = null
      ownerId = userId
      restore()
    },
    { flush: 'sync', immediate: true },
  )

  function current(snapshot: Identity, generation: number) {
    return generation === operationGeneration && identity.isCurrent(snapshot)
  }

  async function retry() {
    if (sending.value || !pending.value) return null
    if (!identity.requireLogin()) return null
    const account = identity.snapshot()
    if (!account || account.userId !== pending.value.owner_user_id) return null
    const snapshot = pending.value
    const generation = ++operationGeneration
    const requestController = new AbortController()
    controller = requestController
    sending.value = true
    error.value = null
    try {
      const result = await createSubmission(
        snapshot.request_key,
        snapshot.body,
        {
          expectedUserId: account.userId,
          signal: requestController.signal,
        },
      )
      if (!current(account, generation)) return null
      clear()
      return result
    } catch (cause) {
      // The API layer converts HTTP, parsing, timeout and network failures.
      if (!(cause instanceof RequestError)) throw cause
      if (!current(account, generation)) return null
      if (identity.handleError(cause, account)) return null
      error.value = cause
      if (
        cause.code !== 'AuthenticationRequired' &&
        cause.code !== 'SessionChanged' &&
        endsCreation(cause)
      )
        clear()
      else if (cause.submissionId) {
        pending.value = freezeRequest(
          snapshot.owner_user_id,
          snapshot.request_key,
          snapshot.body,
          cause.submissionId,
        )
        persist()
      }
      return null
    } finally {
      if (current(account, generation)) {
        sending.value = false
        controller = undefined
      }
    }
  }

  async function start(body: CreateSubmissionRequest) {
    if (sending.value || pending.value) return null
    if (!identity.requireLogin()) return null
    const account = identity.snapshot()
    if (!account || account.userId !== ownerId) return null
    pending.value = freezeRequest(account.userId, crypto.randomUUID(), body)
    restored.value = false
    persist()
    return retry()
  }

  /** Call only after UI confirmation; abandoning cannot cancel a saved submission. */
  function abandonConfirmed() {
    const account = identity.snapshot()
    if (sending.value || !account || account.userId !== ownerId) return false
    clear()
    error.value = null
    return true
  }

  onScopeDispose(() => {
    operationGeneration += 1
    controller?.abort()
  })

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
    // The controller belongs to the application, rather than its first page.
    tabRequest = effectScope(true).run(() =>
      createSubmissionRequest(storage, useAuth()),
    )!
  }
  return tabRequest
}
