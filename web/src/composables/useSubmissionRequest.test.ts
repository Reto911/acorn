import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  createSubmissionRequest,
  PENDING_SUBMISSION_KEY,
} from './useSubmissionRequest'
import type { CreateSubmissionRequest } from '../api/types'

const key = 'd8c5ecb1-d44d-44af-8bed-260c13a4ee19'
const body: CreateSubmissionRequest = {
  problem_id: 1,
  language_id: 'c',
  source: '  // 中文\r\noriginal source\n',
}
const fetchMock = vi.fn<typeof fetch>()

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
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  fetchMock.mockReset()
})

describe('one frozen submission request per tab', () => {
  it('persists before sending, freezes original content and blocks duplicate submissions and abandonment in flight', async () => {
    const storage = sessionStorage()
    const draft = { ...body }
    let finish!: (response: Response) => void
    fetchMock.mockImplementation(() => {
      expect(storage.getItem(PENDING_SUBMISSION_KEY)).toContain(key)
      return new Promise((resolve) => {
        finish = resolve
      })
    })
    const state = createSubmissionRequest(storage)
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
    const state = createSubmissionRequest(storage)
    fetchMock.mockRejectedValueOnce(new TypeError('Network failed'))
    await state.start(body)
    const restored = createSubmissionRequest(storage)
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
    expect(storage.getItem(PENDING_SUBMISSION_KEY)).toBeNull()
    expect(storage.getItem('unrelated-draft')).toBe('keep this draft')
    expect(crypto.randomUUID).toHaveBeenCalledOnce()
  })

  it.each([undefined, 'original-id'])(
    'retains QueueFull, including ID %s, through refresh',
    async (id) => {
      const storage = sessionStorage()
      const state = createSubmissionRequest(storage)
      fetchMock.mockResolvedValue(rejectWith('QueueFull', 503, id))
      await state.start(body)
      expect(state.pending.value).toMatchObject({ request_key: key, body })
      expect(state.pending.value?.submission_id).toBe(id)
      const restored = createSubmissionRequest(storage)
      expect(restored.pending.value).toEqual(state.pending.value)
      expect(fetchMock).toHaveBeenCalledOnce()
    },
  )

  it.each([
    ['BackendStopping', 503],
    ['InternalError', 500],
    ['FutureErrorCode', 400],
    ['toString', 400],
  ])('retains a retryable or uncertain %s response', async (code, status) => {
    const storage = sessionStorage()
    const state = createSubmissionRequest(storage)
    fetchMock.mockResolvedValue(rejectWith(code, status))
    await state.start(body)
    expect(state.pending.value?.request_key).toBe(key)
    expect(storage.getItem(PENDING_SUBMISSION_KEY)).not.toBeNull()
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
      const state = createSubmissionRequest(storage)
      fetchMock.mockResolvedValueOnce(
        rejectWith(code, code === 'IdempotencyConflict' ? 409 : 400),
      )
      await state.start(body)
      expect(state.pending.value).toBeNull()
      expect(state.error.value?.code).toBe(code)
      expect(storage.getItem(PENDING_SUBMISSION_KEY)).toBeNull()
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
      const state = createSubmissionRequest(storage)
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
      expect(storage.getItem(PENDING_SUBMISSION_KEY)).toBeNull()
    },
  )

  it('clears the confirmed abandoned request, preserves unrelated storage and never posts on abandonment', () => {
    const storage = sessionStorage()
    storage.setItem(
      PENDING_SUBMISSION_KEY,
      JSON.stringify({ request_key: key, body }),
    )
    storage.setItem('unrelated-draft', 'draft')
    const state = createSubmissionRequest(storage)
    expect(state.abandonConfirmed()).toBe(true)
    expect(state.pending.value).toBeNull()
    expect(storage.getItem(PENDING_SUBMISSION_KEY)).toBeNull()
    expect(storage.getItem('unrelated-draft')).toBe('draft')
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('allows memory-only retry and reports unavailable or full storage', async () => {
    const storage = sessionStorage()
    storage.setItem.mockImplementation(() => {
      throw new DOMException('private source', 'QuotaExceededError')
    })
    fetchMock.mockRejectedValue(new TypeError('Network failed'))
    const state = createSubmissionRequest(storage)
    await state.start(body)
    expect(state.pending.value?.body).toEqual(body)
    expect(state.storageWarning.value).toContain('内存')
    const unavailable = createSubmissionRequest(null)
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
    storage.setItem(PENDING_SUBMISSION_KEY, 'private source malformed JSON')
    const state = createSubmissionRequest(storage)
    expect(state.storageWarning.value).toContain('无法恢复')
    expect(state.pending.value).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
    expect(JSON.stringify(vi.mocked(console.warn).mock.calls)).not.toContain(
      'private source',
    )
  })
})
