import { effectScope, nextTick } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { listSubmissions } from '../api'
import { useQuery } from '../composables/useQuery'
import { readHistoryQuery } from './historyQuery'

afterEach(() => vi.unstubAllGlobals())

describe('history route queries', () => {
  it('uses backend defaults and preserves allowed filters and pagination across result links', async () => {
    const fetch = vi
      .fn()
      .mockImplementation(async () =>
        Response.json({ items: [], limit: 3, offset: 0 }),
      )
    vi.stubGlobal('fetch', fetch)
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/submissions', component: {} },
        { path: '/submissions/:submissionId', name: 'result', component: {} },
      ],
    })
    await router.push('/submissions?state=&verdict&extra=ignored')
    const scope = effectScope()
    const page = scope.run(() =>
      useQuery(
        () => readHistoryQuery(router.currentRoute.value.query),
        (query, signal) => listSubmissions(query, { signal }),
      ),
    )!
    try {
      await vi.waitFor(() => expect(page.data.value?.limit).toBe(3))
      expect(fetch.mock.calls[0]![0]).toBe('/api/v1/submissions')
      await router.push(
        '/submissions?problem_id=1&state=finished&verdict=AC&limit=3&offset=6&extra=ignored',
      )
      await nextTick()
      await vi.waitFor(() => expect(fetch).toHaveBeenCalledTimes(2))
      expect(fetch.mock.calls[1]![0]).toBe(
        '/api/v1/submissions?limit=3&offset=6&problem_id=1&state=finished&verdict=AC',
      )
    } finally {
      scope.stop()
    }
    const query = readHistoryQuery(router.currentRoute.value.query)
    await router.push({
      name: 'result',
      params: { submissionId: 'existing-record' },
      query: { ...query },
    })
    // Reading the URL after a reload uses the same context, without local storage.
    const directResult = router.resolve(router.currentRoute.value.fullPath)
    const back = router.resolve({
      path: '/submissions',
      query: { ...readHistoryQuery(directResult.query) },
    })
    expect(back.query).toEqual({
      problem_id: '1',
      state: 'finished',
      verdict: 'AC',
      limit: '3',
      offset: '6',
    })
    expect(fetch).toHaveBeenCalledTimes(2)
  })

  it('leaves invalid supplied filters to API rejection rather than silently widening the query', async () => {
    const fetch = vi
      .fn()
      .mockResolvedValue(
        Response.json(
          { error: { code: 'InvalidRequest', message: 'Invalid query' } },
          { status: 400 },
        ),
      )
    vi.stubGlobal('fetch', fetch)
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {})
    try {
      await expect(
        listSubmissions(
          readHistoryQuery({ problem_id: '-1', state: 'unknown', offset: '0' }),
        ),
      ).rejects.toMatchObject({ code: 'InvalidRequest' })
      expect(fetch.mock.calls[0]![0]).toBe(
        '/api/v1/submissions?offset=0&problem_id=-1&state=unknown',
      )
    } finally {
      consoleError.mockRestore()
    }
  })
})
