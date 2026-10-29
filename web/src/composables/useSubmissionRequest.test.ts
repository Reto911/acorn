import { effectScope, ref } from 'vue'
import type { Identity } from './useAuth'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  createSubmissionRequest,
  pendingSubmissionKey,
} from './useSubmissionRequest'
import type { CreateSubmissionRequest } from '../api/types'

const key = 'd8c5ecb1-d44d-44af-8bed-260c13a4ee19'
const body: CreateSubmissionRequest = {
  problem_id: 1,
  language_id: 'c',
  source: '  // 中文\r\noriginal source\n',
}
const fetchMock = vi.fn<typeof fetch>()
const scopes: ReturnType<typeof effectScope>[] = []

function account(initialUserId: number | null = 1) {
  const user = ref(
    initialUserId === null
      ? null
      : {
          id: initialUserId,
          username: `user${initialUserId}`,
          display_name: 'User',
        },
  )
  const generation = ref(0)
  const auth = {
    user,
    generation,
    snapshot: (): Identity | null =>
      user.value
        ? {
            userId: user.value.id,
            generation: generation.value,
          }
        : null,
    isCurrent: (snapshot: Identity) =>
      snapshot.userId === user.value?.id &&
      snapshot.generation === generation.value,
    requireLogin: vi.fn(() => user.value !== null),
    handleError: vi.fn((cause: unknown, snapshot: Identity) => {
      if (!auth.isCurrent(snapshot)) return true
      if (
        typeof cause === 'object' &&
        cause !== null &&
        'code' in cause &&
        ['AuthenticationRequired', 'SessionChanged'].includes(
          String(cause.code),
        )
      ) {
        generation.value += 1
        user.value = null
        return true
      }
      return false
    }),
  }
  function changeUser(id: number | null) {
    generation.value += 1
    user.value =
      id === null
        ? null
        : {
            id,
            username: `user${id}`,
            display_name: 'User',
          }
  }
  return { ...auth, changeUser }
}

function requestState(
  storage: Parameters<typeof createSubmissionRequest>[0],
  identity = account(),
) {
  const scope = effectScope()
  scopes.push(scope)
  return scope.run(() => createSubmissionRequest(storage, identity))!
}

function sessionStorage() {
  const items = new Map<string, string>()
  return {
    getItem: vi.fn((key: string) => items.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => {
      items.set(key, value)
    }),
    removeItem: vi.fn((key: string) => {
      items.delete(key)
    }),
  }
}

function rejectWith(code: string, status: number, submissionId?: string) {
  return Response.json(
    {
      error: {
        code,
        message: 'The operation was rejected.',
        ...(submissionId ? { submission_id: submissionId } : {}),
      },
    },
    { status },
  )
}

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock)
  vi.spyOn(crypto, 'randomUUID').mockReturnValue(key)
  for (const level of ['debug', 'info', 'warn', 'error'] as const)
    vi.spyOn(console, level).mockImplementation(() => {})
})

afterEach(() => {
  for (const scope of scopes.splice(0)) scope.stop()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  fetchMock.mockReset()
})

describe('one frozen submission request per tab', () => {
  it('restores only matching owner snapshots, ignores legacy keys and never posts on account changes', async () => {
    const storage = sessionStorage()
    storage.setItem(
      'acorn:pending-submission:v1',
      JSON.stringify({ request_key: key, body }),
    )
    const identity = account()
    const state = requestState(storage, identity)
    expect(state.pending.value).toBeNull()
    fetchMock.mockRejectedValueOnce(new TypeError('Network failed'))
    await state.start(body)
    expect(state.pending.value?.owner_user_id).toBe(1)
    identity.changeUser(2)
    expect(state.pending.value).toBeNull()
    fetchMock.mockRejectedValueOnce(new TypeError('Network failed'))
    await state.start({ ...body, source: 'other owner' })
    expect(state.pending.value?.owner_user_id).toBe(2)
    identity.changeUser(null)
    expect(state.pending.value).toBeNull()
    expect(await state.retry()).toBeNull()
    expect(await state.start(body)).toBeNull()
    expect(identity.requireLogin).toHaveBeenCalled()
    identity.changeUser(1)
    expect(state.pending.value?.body).toEqual(body)
    expect(state.restored.value).toBe(true)
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(storage.getItem('acorn:pending-submission:v1')).not.toBeNull()
    const otherIdentity = account(3)
    storage.setItem(
      pendingSubmissionKey(3),
      storage.getItem(pendingSubmissionKey(1))!,
    )
    const mismatched = requestState(storage, otherIdentity)
    expect(mismatched.pending.value).toBeNull()
    expect(mismatched.storageWarning.value).toContain('无法恢复')
  })

  it.each([
    ['AuthenticationRequired', 401],
    ['SessionChanged', 409],
  ])(
    'retains the original owner snapshot after %s and requires another manual attempt',
    async (code, status) => {
      const storage = sessionStorage()
      const identity = account()
      const state = requestState(storage, identity)
      fetchMock.mockResolvedValueOnce(rejectWith(code, status))
      await state.start(body)
      expect(state.pending.value).toBeNull()
      expect(
        JSON.parse(storage.getItem(pendingSubmissionKey(1))!).owner_user_id,
      ).toBe(1)
      identity.changeUser(2)
      expect(state.pending.value).toBeNull()
      identity.changeUser(1)
      expect(state.pending.value?.request_key).toBe(key)
      expect(state.pending.value?.body).toEqual(body)
      expect(fetchMock).toHaveBeenCalledOnce()
      fetchMock.mockResolvedValueOnce(
        Response.json({ submission_id: 'saved-id', state: 'queued' }),
      )
      await state.retry()
      expect(fetchMock.mock.calls[1]?.[1]?.headers).toMatchObject({
        'X-Acorn-User-Id': '1',
      })
    },
  )

  it('aborts departing work and ignores its late acknowledgement without clearing the new owner attempt', async () => {
    const storage = sessionStorage()
    const identity = account()
    const state = requestState(storage, identity)
    let finish!: (value: Response) => void
    fetchMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve
        }),
    )
    const first = state.start(body)
    const oldSignal = fetchMock.mock.calls[0]?.[1]?.signal
    identity.changeUser(2)
    expect(oldSignal?.aborted).toBe(true)
    expect(state.sending.value).toBe(false)
    fetchMock.mockRejectedValueOnce(new TypeError('Network failed'))
    await state.start({ ...body, source: 'new owner' })
    finish(Response.json({ submission_id: 'old-result', state: 'queued' }))
    expect(await first).toBeNull()
    expect(state.pending.value?.owner_user_id).toBe(2)
    expect(state.pending.value?.body.source).toBe('new owner')
    expect(storage.getItem(pendingSubmissionKey(1))).not.toBeNull()
    expect(storage.getItem(pendingSubmissionKey(2))).not.toBeNull()
    expect(state.sending.value).toBe(false)
  })

  it('persists before sending, freezes original content and blocks duplicate submissions and abandonment in flight', async () => {
    const storage = sessionStorage()
    const draft = { ...body }
    let finish!: (response: Response) => void
    fetchMock.mockImplementation(() => {
      expect(storage.getItem(pendingSubmissionKey(1))).toContain(key)
      return new Promise((resolve) => {
        finish = resolve
      })
    })
    const state = requestState(storage)
    const first = state.start(draft)
    expect(state.sending.value).toBe(true)
    expect(Object.isFrozen(state.pending.value)).toBe(true)
    expect(Object.isFrozen(state.pending.value?.body)).toBe(true)
    draft.source = 'newly edited source'
    expect(await state.start(draft)).toBeNull()
    expect(await state.retry()).toBeNull()
    expect(state.abandonConfirmed()).toBe(false)
    expect(fetchMock).toHaveBeenCalledOnce()
    expect(crypto.randomUUID).toHaveBeenCalledOnce()
    expect(state.pending.value?.body.source).toBe(body.source)
    finish(
      Response.json(
        { submission_id: 'saved-id', state: 'queued' },
        { status: 202 },
      ),
    )
    await first
    expect(state.sending.value).toBe(false)
    expect(state.pending.value).toBeNull()
  })

  it('restores without posting, then manually retries the same UUID and exact original bytes', async () => {
    const storage = sessionStorage()
    storage.setItem('unrelated-draft', 'keep this draft')
    const state = requestState(storage)
    fetchMock.mockRejectedValueOnce(new TypeError('Network failed'))
    await state.start(body)
    const restored = requestState(storage)
    expect(restored.restored.value).toBe(true)
    expect(restored.pending.value?.body).toEqual(body)
    expect(fetchMock).toHaveBeenCalledOnce()
    fetchMock.mockResolvedValueOnce(
      Response.json({ submission_id: 'saved-id', state: 'finished' }),
    )
    await restored.retry()
    const [first, second] = fetchMock.mock.calls.map((call) => call[1]!)
    expect(second.body).toBe(first.body)
    expect(second.headers).toEqual(first.headers)
    expect(restored.pending.value).toBeNull()
    expect(storage.getItem(pendingSubmissionKey(1))).toBeNull()
    expect(storage.getItem('unrelated-draft')).toBe('keep this draft')
    expect(crypto.randomUUID).toHaveBeenCalledOnce()
  })

  it.each([undefined, 'original-id'])(
    'retains QueueFull, including ID %s, through refresh',
    async (id) => {
      const storage = sessionStorage()
      const state = requestState(storage)
      fetchMock.mockResolvedValue(rejectWith('QueueFull', 503, id))
      await state.start(body)
      expect(state.pending.value).toMatchObject({ request_key: key, body })
      expect(state.pending.value?.submission_id).toBe(id)
      const restored = requestState(storage)
      expect(restored.pending.value).toEqual(state.pending.value)
      expect(fetchMock).toHaveBeenCalledOnce()
    },
  )

  it.each([
    ['BackendStopping', 503],
    ['WorkersUnavailable', 503],
    ['InternalError', 500],
    ['FutureErrorCode', 400],
    ['toString', 400],
  ])('retains a retryable or uncertain %s response', async (code, status) => {
    const storage = sessionStorage()
    const state = requestState(storage)
    fetchMock.mockResolvedValue(rejectWith(code, status))
    await state.start(body)
    expect(state.pending.value?.request_key).toBe(key)
    expect(storage.getItem(pendingSubmissionKey(1))).not.toBeNull()
  })

  it.each([
    'InvalidRequest',
    'SourceTooLarge',
    'LanguageNotAllowed',
    'IdempotencyConflict',
  ])(
    'ends a definitive %s refusal and permits a new active attempt',
    async (code) => {
      const storage = sessionStorage()
      const state = requestState(storage)
      fetchMock.mockResolvedValueOnce(
        rejectWith(code, code === 'IdempotencyConflict' ? 409 : 400),
      )
      await state.start(body)
      expect(state.pending.value).toBeNull()
      expect(state.error.value?.code).toBe(code)
      expect(storage.getItem(pendingSubmissionKey(1))).toBeNull()
      fetchMock.mockResolvedValueOnce(
        Response.json(
          { submission_id: 'new-id', state: 'queued' },
          { status: 202 },
        ),
      )
      await state.start({ ...body, source: 'corrected' })
      expect(crypto.randomUUID).toHaveBeenCalledTimes(2)
    },
  )

  it.each([200, 202])(
    'clears only a valid status %s acknowledgement',
    async (status) => {
      const storage = sessionStorage()
      const state = requestState(storage)
      fetchMock.mockResolvedValueOnce(Response.json({}, { status }))
      await state.start(body)
      expect(state.pending.value).not.toBeNull()
      fetchMock.mockResolvedValueOnce(
        Response.json(
          { submission_id: 'saved-id', state: 'queued' },
          { status },
        ),
      )
      expect(await state.retry()).toEqual({
        submission_id: 'saved-id',
        state: 'queued',
      })
      expect(state.pending.value).toBeNull()
      expect(storage.getItem(pendingSubmissionKey(1))).toBeNull()
    },
  )

  it('clears the confirmed abandoned request, preserves unrelated storage and never posts on abandonment', () => {
    const storage = sessionStorage()
    storage.setItem(
      pendingSubmissionKey(1),
      JSON.stringify({ owner_user_id: 1, request_key: key, body }),
    )
    storage.setItem('unrelated-draft', 'draft')
    const state = requestState(storage)
    expect(state.abandonConfirmed()).toBe(true)
    expect(state.pending.value).toBeNull()
    expect(storage.getItem(pendingSubmissionKey(1))).toBeNull()
    expect(storage.getItem('unrelated-draft')).toBe('draft')
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('allows memory-only retry and reports unavailable or full storage', async () => {
    const storage = sessionStorage()
    storage.setItem.mockImplementation(() => {
      throw new DOMException('private source', 'QuotaExceededError')
    })
    fetchMock.mockRejectedValue(new TypeError('Network failed'))
    const state = requestState(storage)
    await state.start(body)
    expect(state.pending.value?.body).toEqual(body)
    expect(state.storageWarning.value).toContain('内存')
    const unavailable = requestState(null)
    await unavailable.start(body)
    expect(unavailable.pending.value?.body).toEqual(body)
    expect(unavailable.storageWarning.value).toContain('刷新')
    const logs = JSON.stringify(vi.mocked(console.warn).mock.calls)
    expect(logs).not.toContain(body.source)
    expect(logs).not.toContain(key)
    expect(logs).not.toContain('private source')
  })

  it('reports corrupt saved data without logging it or silently sending a new request', () => {
    const storage = sessionStorage()
    storage.setItem(pendingSubmissionKey(1), 'private source malformed JSON')
    const state = requestState(storage)
    expect(state.storageWarning.value).toContain('无法恢复')
    expect(state.pending.value).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
    expect(JSON.stringify(vi.mocked(console.warn).mock.calls)).not.toContain(
      'private source',
    )
  })
})
