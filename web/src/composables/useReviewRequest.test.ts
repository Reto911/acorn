import { effectScope, shallowRef, type EffectScope } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { RequestError } from '../api/http'
import type { startReview } from '../api'
import type { ReviewResponse, Submission } from '../api/types'
import { useReviewRequest } from './useReviewRequest'

const finished: Submission = {
  submission_id: 'submission-a',
  problem_id: 1,
  version_no: 1,
  language_id: 'c',
  state: 'finished',
  verdict: 'CE',
  created_at_ms: 1000,
  terminal_at_ms: 2000,
  terminal_details: {
    compile_diagnostic_base64: '',
    compile_diagnostic_truncated: false,
  },
  review_status: 'DISABLED',
  review: null,
}
const accepted: ReviewResponse = {
  submission_id: finished.submission_id,
  review_status: 'PENDING',
  review: {
    review_id: 'review-a',
    trigger: 'manual',
    model: 'review-model',
    requested_at_ms: 3000,
    completed_at_ms: null,
    result: null,
    error: null,
  },
}
const scopes: EffectScope[] = []
const context = {
  operation: 'start-review',
  path: '/api/v1/submissions/submission-a/review',
  method: 'POST' as const,
}

function scope() {
  const value = effectScope()
  scopes.push(value)
  return value
}

beforeEach(() => vi.useFakeTimers())
afterEach(() => {
  for (const owner of scopes.splice(0)) owner.stop()
  vi.useRealTimers()
})

describe('manual review requests', () => {
  it('only posts on explicit action, prevents double clicks and passes the latest expected identity', async () => {
    const submission = shallowRef<Submission>(finished)
    let resolve!: (response: ReviewResponse) => void
    const send = vi
      .fn()
      .mockReturnValueOnce(
        new Promise<ReviewResponse>((res) => {
          resolve = res
        }),
      )
      .mockResolvedValueOnce(accepted)
    const reconcile = vi.fn(async (response?: ReviewResponse) => {
      if (response) submission.value = { ...submission.value, ...response }
    })
    const request = scope().run(() =>
      useReviewRequest(
        () => submission.value,
        () => false,
        reconcile,
        send,
      ),
    )!
    await vi.advanceTimersByTimeAsync(10000)
    expect(send).not.toHaveBeenCalled()
    expect(request.canStart.value).toBe(true)
    const waiting = request.start()
    expect(request.sending.value).toBe(true)
    await request.start()
    expect(send).toHaveBeenCalledOnce()
    expect(send).toHaveBeenCalledWith('submission-a', null, {
      signal: expect.any(AbortSignal),
    })
    resolve(accepted)
    await waiting
    expect(reconcile).toHaveBeenCalledWith(accepted)
    expect(request.canStart.value).toBe(false)
    submission.value = {
      ...finished,
      review_status: 'FAILED',
      review: {
        ...accepted.review,
        completed_at_ms: 4000,
        error: { code: 'ReviewTimeout', message: 'Timed out.' },
      },
    }
    await request.start()
    expect(send.mock.calls[1]![1]).toBe('review-a')
    expect(send).toHaveBeenCalledTimes(2)
  })

  it.each([
    new RequestError('network', context),
    new RequestError('http', context, 502),
    new RequestError('http', context, 409, {
      code: 'ReviewChanged',
      message: 'Changed.',
    }),
  ])(
    'queries uncertain or stale outcomes without reposting',
    async (failure) => {
      const refreshRequired = shallowRef(false)
      const send = vi.fn().mockRejectedValue(failure)
      const reconcile = vi.fn(async () => {
        refreshRequired.value = true
      })
      const request = scope().run(() =>
        useReviewRequest(
          () => finished,
          () => refreshRequired.value,
          reconcile,
          send,
        ),
      )!
      await request.start()
      expect(request.error.value).toBe(failure)
      expect(reconcile).toHaveBeenCalledOnce()
      expect(reconcile).toHaveBeenCalledWith()
      expect(request.canStart.value).toBe(false)
      await request.start()
      await vi.advanceTimersByTimeAsync(10000)
      expect(send).toHaveBeenCalledOnce()
      // Only a successful query, represented by clearing this gate, enables another explicit action.
      refreshRequired.value = false
      expect(request.canStart.value).toBe(true)
      expect(send).toHaveBeenCalledOnce()
    },
  )

  it('preserves the previous review on definitive rejection and disables non-finished or pending submissions', async () => {
    const previous: Submission = {
      ...finished,
      review_status: 'PASS',
      review: {
        ...accepted.review,
        completed_at_ms: 4000,
        result: { decision: 'PASS', summary: '通用算法', findings: [] },
      },
    }
    const submission = shallowRef<Submission>(previous)
    const send = vi.fn().mockRejectedValue(
      new RequestError('http', context, 503, {
        code: 'ReviewQueueFull',
        message: 'Full.',
      }),
    )
    const reconcile = vi.fn(async () => {})
    const request = scope().run(() =>
      useReviewRequest(
        () => submission.value,
        () => false,
        reconcile,
        send,
      ),
    )!
    await request.start()
    expect(submission.value).toBe(previous)
    expect(reconcile).not.toHaveBeenCalled()
    expect(request.canStart.value).toBe(true)
    submission.value = { ...finished, ...accepted }
    await request.start()
    expect(request.canStart.value).toBe(false)
    submission.value = {
      ...finished,
      state: 'failed',
      verdict: null,
      terminal_details: {
        code: 'SchedulerInterrupted',
        diagnostic: 'Interrupted.',
      },
    }
    await request.start()
    expect(request.canStart.value).toBe(false)
    submission.value = {
      ...finished,
      state: 'running',
      verdict: null,
      terminal_details: null,
      terminal_at_ms: null,
    }
    await request.start()
    expect(send).toHaveBeenCalledOnce()
  })

  it('cancels browser waits on ID changes and disposal, ignoring late admission responses', async () => {
    const submission = shallowRef<Submission>(finished)
    const resolvers: ((response: ReviewResponse) => void)[] = []
    const send = vi.fn<typeof startReview>().mockImplementation(
      () =>
        new Promise<ReviewResponse>((resolve) => {
          resolvers.push(resolve)
        }),
    )
    const reconcile = vi.fn(async () => {})
    const owner = scope()
    const request = owner.run(() =>
      useReviewRequest(
        () => submission.value,
        () => false,
        reconcile,
        send,
      ),
    )!
    const first = request.start()
    submission.value = { ...finished, submission_id: 'submission-b' }
    expect(send.mock.calls[0]![2]?.signal?.aborted).toBe(true)
    const second = request.start()
    resolvers[0]!(accepted)
    await first
    expect(reconcile).not.toHaveBeenCalled()
    expect(request.sending.value).toBe(true)
    owner.stop()
    expect(send.mock.calls[1]![2]?.signal?.aborted).toBe(true)
    resolvers[1]!({ ...accepted, submission_id: 'submission-b' })
    await second
    expect(reconcile).not.toHaveBeenCalled()
  })

  it.each([
    { status: 401, code: 'AuthenticationRequired' },
    { status: 409, code: 'SessionChanged' },
  ])(
    'stops on $code without reconciliation or reposting',
    async ({ status, code }) => {
      const failure = new RequestError('http', context, status, {
        code,
        message: 'Identity rejected.',
      })
      const send = vi.fn().mockRejectedValue(failure)
      const reconcile = vi.fn(async () => {})
      const handleError = vi.fn(() => true)
      const request = scope().run(() =>
        useReviewRequest(
          () => finished,
          () => false,
          reconcile,
          send,
          { expectedUserId: 42, handleError },
        ),
      )!
      await request.start()
      expect(send).toHaveBeenCalledWith('submission-a', null, {
        signal: expect.any(AbortSignal),
        expectedUserId: 42,
      })
      expect(handleError).toHaveBeenCalledWith(failure)
      expect(request.error.value).toBeNull()
      expect(request.canStart.value).toBe(false)
      await request.start()
      await vi.advanceTimersByTimeAsync(10_000)
      expect(send).toHaveBeenCalledOnce()
      expect(reconcile).not.toHaveBeenCalled()
    },
  )

  it('discards a late review admission when the owner is invalidated', async () => {
    let resolve!: (response: ReviewResponse) => void
    const send = vi.fn().mockReturnValue(
      new Promise<ReviewResponse>((res) => {
        resolve = res
      }),
    )
    const reconcile = vi.fn(async () => {})
    const request = scope().run(() =>
      useReviewRequest(
        () => finished,
        () => false,
        reconcile,
        send,
      ),
    )!
    const waiting = request.start()
    request.stop()
    expect(send.mock.calls[0]![2].signal.aborted).toBe(true)
    resolve(accepted)
    await waiting
    await request.start()
    expect(send).toHaveBeenCalledOnce()
    expect(reconcile).not.toHaveBeenCalled()
  })
})
