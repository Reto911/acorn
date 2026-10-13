import { effectScope, nextTick, ref, type EffectScope } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Review, Submission } from '../api/types'
import { useSubmissionPolling } from './useSubmissionPolling'

const queued: Submission = {
  submission_id: 'first',
  problem_id: 1,
  version_no: 1,
  language_id: 'c',
  state: 'queued',
  verdict: null,
  review_status: 'DISABLED',
  review: null,
  created_at_ms: 1000,
  terminal_at_ms: null,
  terminal_details: null,
}

class Visibility extends EventTarget {
  hidden = false
  change(hidden: boolean) {
    this.hidden = hidden
    this.dispatchEvent(new Event('visibilitychange'))
  }
}

function deferred() {
  let resolve!: (snapshot: Submission) => void
  const promise = new Promise<Submission>((res) => {
    resolve = res
  })
  return { promise, resolve }
}

const scopes: EffectScope[] = []
function scope() {
  const value = effectScope()
  scopes.push(value)
  return value
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.spyOn(console, 'info').mockImplementation(() => {})
})
afterEach(() => {
  for (const value of scopes.splice(0)) value.stop()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('submission polling lifecycle', () => {
  it('waits one second after completion, never overlaps requests, and stops at a terminal snapshot', async () => {
    const slow = deferred()
    const load = vi
      .fn<(_id: string, signal: AbortSignal) => Promise<Submission>>()
      .mockReturnValueOnce(slow.promise)
      .mockResolvedValueOnce({ ...queued, state: 'dispatching' })
      .mockResolvedValueOnce({ ...queued, state: 'running' })
      .mockResolvedValueOnce({
        ...queued,
        state: 'finished',
        verdict: 'AC',
        terminal_at_ms: 9000,
      })
    const query = scope().run(() =>
      useSubmissionPolling(() => 'first', load, new Visibility()),
    )!
    await vi.advanceTimersByTimeAsync(5000)
    await query.refresh()
    expect(load).toHaveBeenCalledTimes(1)
    slow.resolve(queued)
    await nextTick()
    await vi.advanceTimersByTimeAsync(999)
    expect(load).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1)
    expect(query.data.value?.state).toBe('dispatching')
    await vi.advanceTimersByTimeAsync(1000)
    expect(query.data.value?.state).toBe('running')
    await vi.advanceTimersByTimeAsync(1000)
    expect(query.terminal.value).toBe(true)
    await vi.advanceTimersByTimeAsync(10000)
    expect(load).toHaveBeenCalledTimes(4)
  })

  it('pauses and cancels while hidden; a query failure keeps the snapshot paused until manual retry', async () => {
    const visibility = new Visibility()
    const hiddenRequest = deferred()
    const load = vi
      .fn<(_id: string, signal: AbortSignal) => Promise<Submission>>()
      .mockResolvedValueOnce(queued)
      .mockReturnValueOnce(hiddenRequest.promise)
      .mockRejectedValueOnce(new Error('Query failed'))
      .mockResolvedValueOnce({
        ...queued,
        state: 'failed',
        terminal_at_ms: 3000,
        terminal_details: {
          code: 'SchedulerInterrupted',
          diagnostic: 'Interrupted',
        },
      })
    const owner = scope()
    const query = owner.run(() =>
      useSubmissionPolling(() => 'first', load, visibility),
    )!
    await nextTick()
    visibility.change(true)
    await vi.advanceTimersByTimeAsync(5000)
    expect(load).toHaveBeenCalledTimes(1)
    visibility.change(false)
    expect(load).toHaveBeenCalledTimes(2)
    visibility.change(true)
    expect(load.mock.calls[1]![1].aborted).toBe(true)
    hiddenRequest.resolve({ ...queued, state: 'running' })
    await nextTick()
    expect(query.data.value?.state).toBe('queued')
    expect(query.error.value).toBeNull()
    visibility.change(false)
    await nextTick()
    expect(query.error.value).toBeInstanceOf(Error)
    expect(query.data.value?.state).toBe('queued')
    visibility.change(true)
    visibility.change(false)
    await vi.advanceTimersByTimeAsync(5000)
    expect(load).toHaveBeenCalledTimes(3)
    await query.refresh()
    expect(query.data.value?.state).toBe('failed')
    expect(query.error.value).toBeNull()
    visibility.change(true)
    visibility.change(false)
    await vi.advanceTimersByTimeAsync(5000)
    expect(load).toHaveBeenCalledTimes(4)
    owner.stop()
    visibility.change(true)
    visibility.change(false)
    expect(load).toHaveBeenCalledTimes(4)
  })

  it('cancels changed IDs and disposal and ignores their late responses', async () => {
    const id = ref('first')
    const first = deferred()
    const second = deferred()
    const load = vi
      .fn<(_id: string, signal: AbortSignal) => Promise<Submission>>()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
    const owner = scope()
    const query = owner.run(() =>
      useSubmissionPolling(() => id.value, load, new Visibility()),
    )!
    id.value = 'second'
    await nextTick()
    expect(load.mock.calls[0]![1].aborted).toBe(true)
    expect(load.mock.calls[1]![0]).toBe('second')
    first.resolve(queued)
    await nextTick()
    expect(query.data.value).toBeNull()
    owner.stop()
    expect(load.mock.calls[1]![1].aborted).toBe(true)
    second.resolve({ ...queued, submission_id: 'second' })
    await nextTick()
    await vi.advanceTimersByTimeAsync(5000)
    expect(query.data.value).toBeNull()
    expect(load).toHaveBeenCalledTimes(2)
  })

  it('keeps polling a finished submission while review is pending and resumes after manual acceptance', async () => {
    const review: Review = {
      review_id: 'review-a',
      trigger: 'manual',
      requested_at_ms: 3000,
      completed_at_ms: null,
      model: 'review-model',
      result: null,
      error: null,
    }
    const finished: Submission = {
      ...queued,
      state: 'finished',
      verdict: 'AC',
      terminal_at_ms: 2000,
    }
    const pending: Submission = {
      ...finished,
      review_status: 'PENDING',
      review,
    }
    const completed: Submission = {
      ...finished,
      review_status: 'PASS',
      review: {
        ...review,
        completed_at_ms: 4000,
        result: { decision: 'PASS', summary: '通用算法', findings: [] },
      },
    }
    const old = deferred()
    const load = vi
      .fn<(_id: string, signal: AbortSignal) => Promise<Submission>>()
      .mockResolvedValueOnce(finished)
      .mockReturnValueOnce(old.promise)
      .mockResolvedValueOnce(pending)
      .mockResolvedValueOnce(pending)
      .mockResolvedValueOnce(completed)
    const query = scope().run(() =>
      useSubmissionPolling(() => 'first', load, new Visibility()),
    )!
    await nextTick()
    expect(query.terminal.value).toBe(true)
    const oldWait = query.refresh()
    await query.reconcileReview({
      submission_id: 'first',
      review_status: 'PENDING',
      review,
    })
    expect(load.mock.calls[1]![1].aborted).toBe(true)
    old.resolve(finished)
    await oldWait
    expect(query.data.value?.review_status).toBe('PENDING')
    expect(query.terminal.value).toBe(false)
    await vi.advanceTimersByTimeAsync(2000)
    expect(query.data.value?.review_status).toBe('PASS')
    expect(query.terminal.value).toBe(true)
    await vi.advanceTimersByTimeAsync(5000)
    expect(load).toHaveBeenCalledTimes(5)
  })

  it('requires a successful GET after an uncertain POST, including hidden and failed queries', async () => {
    const visibility = new Visibility()
    const finished: Submission = {
      ...queued,
      state: 'finished',
      verdict: 'AC',
      terminal_at_ms: 2000,
    }
    const load = vi
      .fn<(_id: string, signal: AbortSignal) => Promise<Submission>>()
      .mockResolvedValueOnce(finished)
      .mockRejectedValueOnce(new Error('Query failed'))
      .mockResolvedValueOnce(finished)
    const query = scope().run(() =>
      useSubmissionPolling(() => 'first', load, visibility),
    )!
    await nextTick()
    visibility.change(true)
    await query.reconcileReview()
    expect(query.refreshRequired.value).toBe(true)
    expect(load).toHaveBeenCalledTimes(1)
    visibility.change(false)
    await nextTick()
    expect(query.refreshRequired.value).toBe(true)
    expect(query.error.value).toBeInstanceOf(Error)
    visibility.change(true)
    visibility.change(false)
    await vi.advanceTimersByTimeAsync(5000)
    expect(load).toHaveBeenCalledTimes(2)
    await query.refresh()
    expect(query.refreshRequired.value).toBe(false)
    expect(query.terminal.value).toBe(true)
  })
})
