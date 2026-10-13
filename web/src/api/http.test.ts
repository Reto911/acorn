import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { RequestError, requestJson } from './http'
import {
  createSubmission,
  getSubmission,
  listProblems,
  listSubmissions,
  startReview,
} from '.'
import { errorFeedback } from './feedback'

const fetchMock = vi.fn<typeof fetch>()
const path = '/api/v1/problems'

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock)
  for (const level of ['debug', 'info', 'warn', 'error'] as const)
    vi.spyOn(console, level).mockImplementation(() => {})
})

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  fetchMock.mockReset()
})

describe('HTTP operation boundaries', () => {
  it('extracts HTTP error context and an existing queued ID', async () => {
    fetchMock.mockResolvedValue(
      Response.json(
        {
          error: {
            code: 'QueueFull',
            message: 'The submission queue is full.',
            submission_id: 'saved-id',
          },
        },
        { status: 503 },
      ),
    )
    const error = await requestJson(path, { operation: 'test' }).catch(
      (error: unknown) => error,
    )
    expect(error).toBeInstanceOf(RequestError)
    expect(error).toMatchObject({
      kind: 'http',
      status: 503,
      code: 'QueueFull',
      submissionId: 'saved-id',
    })
    expect(errorFeedback(error, 'create').title).toContain('已保存')
    expect(console.warn).toHaveBeenCalledOnce()
  })

  it('treats a stored execution failure as a successful query', async () => {
    const submission = {
      submission_id: 'saved-id',
      state: 'failed',
      verdict: null,
      terminal_details: {
        code: 'SchedulerInterrupted',
        diagnostic: 'The backend restarted.',
      },
    }
    fetchMock.mockResolvedValue(Response.json(submission))
    expect(await getSubmission('saved-id')).toEqual(submission)
    expect(console.error).not.toHaveBeenCalled()
    expect(console.warn).not.toHaveBeenCalled()
  })

  it.each([200, 502])(
    'handles non-JSON status %s without logging bytes',
    async (status) => {
      const secret = 'private source and output'
      fetchMock.mockResolvedValue(
        new Response(`<html>${secret}</html>`, { status }),
      )
      const error = await requestJson(path, { operation: 'test' }).catch(
        (error: unknown) => error,
      )
      expect(error).toMatchObject({ kind: 'invalid-response', status })
      expect(errorFeedback(error).title).toContain('无法读取')
      expect(JSON.stringify(vi.mocked(console.error).mock.calls)).not.toContain(
        secret,
      )
    },
  )

  it('reports network errors without reflecting request content or cause text', async () => {
    fetchMock.mockRejectedValue(new TypeError('private source text'))
    const error = await requestJson(path, { operation: 'test' }).catch(
      (error: unknown) => error,
    )
    expect(error).toMatchObject({ kind: 'network' })
    expect(errorFeedback(error, 'create').title).toContain('尚未确认')
    expect(JSON.stringify(vi.mocked(console.error).mock.calls)).not.toContain(
      'private source',
    )
  })

  it('times out at ten seconds and removes cancellation listeners', async () => {
    vi.useFakeTimers()
    const caller = new AbortController()
    const removeListener = vi.spyOn(caller.signal, 'removeEventListener')
    fetchMock.mockImplementation(
      (_input, options) =>
        new Promise((_resolve, reject) => {
          options?.signal?.addEventListener('abort', () =>
            reject(new DOMException('Aborted', 'AbortError')),
          )
        }),
    )
    const result = requestJson(path, {
      operation: 'test',
      signal: caller.signal,
    }).catch((error: unknown) => error)
    await vi.advanceTimersByTimeAsync(9_999)
    expect(fetchMock.mock.calls[0]![1]?.signal?.aborted).toBe(false)
    await vi.advanceTimersByTimeAsync(1)
    expect(await result).toMatchObject({ kind: 'timeout' })
    expect(removeListener).toHaveBeenCalledWith('abort', expect.any(Function))
    expect(vi.getTimerCount()).toBe(0)
  })

  it('keeps the timeout active while the response body is still arriving', async () => {
    vi.useFakeTimers()
    fetchMock.mockImplementation(
      async (_input, options) =>
        ({
          status: 200,
          ok: true,
          text: () =>
            new Promise<string>((_resolve, reject) => {
              options?.signal?.addEventListener('abort', () =>
                reject(new Error('Aborted')),
              )
            }),
        }) as Response,
    )
    const result = requestJson(path, { operation: 'test' }).catch(
      (error: unknown) => error,
    )
    await vi.advanceTimersByTimeAsync(10_000)
    expect(await result).toMatchObject({ kind: 'timeout', status: 200 })
  })

  it('does not fetch or report a request error when already cancelled', async () => {
    const caller = new AbortController()
    caller.abort()
    await expect(
      requestJson(path, { operation: 'test', signal: caller.signal }),
    ).rejects.toMatchObject({ kind: 'cancelled' })
    expect(fetchMock).not.toHaveBeenCalled()
    expect(console.error).not.toHaveBeenCalled()
  })
})

describe('allowed API fields', () => {
  it('sends only the expected review identity and accepts both new and reused reviews', async () => {
    const review = {
      review_id: 'review-a',
      trigger: 'manual',
      model: 'review-model',
      requested_at_ms: 1000,
      completed_at_ms: null,
      result: null,
      error: null,
    }
    const response = {
      submission_id: 'saved-id',
      review_status: 'PENDING',
      review,
    }
    fetchMock
      .mockResolvedValueOnce(Response.json(response, { status: 202 }))
      .mockResolvedValueOnce(Response.json(response, { status: 200 }))
    expect(await startReview('saved-id', null)).toEqual(response)
    expect(await startReview('saved-id', 'review-a')).toEqual(response)
    expect(
      fetchMock.mock.calls.map(([url, options]) => ({
        url,
        method: options?.method,
        body: JSON.parse(options?.body as string),
      })),
    ).toEqual([
      {
        url: '/api/v1/submissions/saved-id/review',
        method: 'POST',
        body: { expected_review_id: null },
      },
      {
        url: '/api/v1/submissions/saved-id/review',
        method: 'POST',
        body: { expected_review_id: 'review-a' },
      },
    ])
    expect(fetchMock.mock.calls[0]![1]?.headers).not.toHaveProperty(
      'Idempotency-Key',
    )
  })

  it('treats a malformed review acknowledgement as uncertain without repeating POST', async () => {
    fetchMock.mockResolvedValue(
      Response.json(
        {
          submission_id: 'other-id',
          review_status: 'PENDING',
          review: { review_id: 'review-a' },
        },
        { status: 202 },
      ),
    )
    const error = await startReview('saved-id', null).catch(
      (cause: unknown) => cause,
    )
    expect(error).toMatchObject({ kind: 'invalid-response', status: 202 })
    expect(errorFeedback(error, 'review').title).toContain('尚未确认审查')
    expect(fetchMock).toHaveBeenCalledOnce()
  })

  it('picks list parameters and preserves backend defaults', async () => {
    fetchMock.mockImplementation(async () =>
      Response.json({ items: [], limit: 20, offset: 0 }),
    )
    await listProblems()
    await listProblems({
      limit: 2,
      offset: 0,
      problem_id: 9,
      extra: 'unallowed',
    } as never)
    await listSubmissions({
      problem_id: 1,
      state: 'finished',
      verdict: 'AC',
      review_status: 'DOUBTFUL',
      limit: 2,
      offset: 0,
      extra: true,
    } as never)
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      '/api/v1/problems',
      '/api/v1/problems?limit=2&offset=0',
      '/api/v1/submissions?limit=2&offset=0&problem_id=1&state=finished&verdict=AC&review_status=DOUBTFUL',
    ])
  })

  it.each([200, 202])(
    'accepts status %s and sends only the original UTF-8 content',
    async (status) => {
      fetchMock.mockResolvedValue(
        Response.json(
          { submission_id: 'saved-id', state: 'queued' },
          { status },
        ),
      )
      const source = '  // 中文\r\nint main(void) { return 0; }\n'
      await createSubmission('original-key', {
        problem_id: 1,
        language_id: 'c',
        source,
        version_no: 99,
        user_id: 9,
      } as never)
      const options = fetchMock.mock.calls[0]![1]!
      expect(options.headers).toMatchObject({
        'Content-Type': 'application/json',
        'Idempotency-Key': 'original-key',
      })
      expect(JSON.parse(options.body as string)).toEqual({
        problem_id: 1,
        language_id: 'c',
        source,
      })
      const logs = JSON.stringify(vi.mocked(console.info).mock.calls)
      expect(logs).not.toContain('original-key')
      expect(logs).not.toContain(source)
      expect(console.info).toHaveBeenCalledWith(
        '[Acorn]',
        expect.objectContaining({
          status,
          submission_id: 'saved-id',
          duration_ms: expect.any(Number),
        }),
      )
    },
  )

  it.each([
    [200, {}],
    [202, { submission_id: '', state: 'queued' }],
    [200, { submission_id: 'id', state: 'unknown' }],
    [201, { submission_id: 'id', state: 'queued' }],
  ])(
    'does not acknowledge malformed creation status %s',
    async (status, body) => {
      fetchMock.mockResolvedValue(Response.json(body, { status }))
      await expect(
        createSubmission('key', {
          problem_id: 1,
          language_id: 'c',
          source: 'x',
        }),
      ).rejects.toMatchObject({ kind: 'invalid-response' })
    },
  )
})
