import { effectScope, nextTick, ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { useQuery } from './useQuery'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('page query lifecycle', () => {
  it('cancels changed identity/query requests and ignores late results after navigation or disposal', async () => {
    const route = ref({ id: 'first', offset: 0 })
    const first = deferred<string>()
    const second = deferred<string>()
    const third = deferred<string>()
    const load = vi
      .fn<
        (_query: typeof route.value, signal: AbortSignal) => Promise<string>
      >()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
      .mockReturnValueOnce(third.promise)
    const scope = effectScope()
    const query = scope.run(() => useQuery(() => route.value, load))!
    expect(query.loading.value).toBe(true)
    route.value = { id: 'second', offset: 0 }
    await nextTick()
    expect(load.mock.calls[0]![1].aborted).toBe(true)
    second.resolve('current result')
    await second.promise
    first.resolve('late stale result')
    await first.promise
    expect(query.data.value).toBe('current result')
    route.value.offset = 20
    await nextTick()
    expect(query.data.value).toBeNull()
    scope.stop()
    expect(load.mock.calls[2]![1].aborted).toBe(true)
    third.resolve('unmounted result')
    await third.promise
    expect(query.data.value).toBeNull()
  })

  it('keeps the last success on refresh failure and recovers only on manual reload', async () => {
    const load = vi
      .fn()
      .mockResolvedValueOnce('last success')
      .mockRejectedValueOnce(new Error('Query failed'))
      .mockResolvedValueOnce('new result')
    const scope = effectScope()
    const query = scope.run(() => useQuery(() => 'id', load))!
    await nextTick()
    expect(query.data.value).toBe('last success')
    await query.reload()
    expect(query.data.value).toBe('last success')
    expect(query.error.value).toBeInstanceOf(Error)
    expect(query.loading.value).toBe(false)
    await nextTick()
    expect(load).toHaveBeenCalledTimes(2)
    await query.reload()
    expect(query.data.value).toBe('new result')
    expect(query.error.value).toBeNull()
    scope.stop()
  })
})
