import { effectScope, ref } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Problem } from '../api/types'
import { useDraft } from './useDraft'

const problem: Problem = {
  problem_id: 1,
  version_no: 1,
  title: 'A + B',
  statement: 'Add two integers.',
  languages: [
    {
      language_id: 'c',
      cpu_time_ms: 1000,
      wall_time_ms: 3000,
      memory_kib: 262144,
    },
    {
      language_id: 'python',
      cpu_time_ms: 2000,
      wall_time_ms: 5000,
      memory_kib: 262144,
    },
  ],
}

function memoryStorage() {
  const values = new Map<string, string>()
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: vi.fn((key: string, value: string) => {
      values.set(key, value)
    }),
    removeItem: (key: string) => {
      values.delete(key)
    },
  }
}

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('local answer drafts', () => {
  it('debounces writes, flushes on switch and disposal, and restores independent problem/language drafts', () => {
    vi.useFakeTimers()
    const storage = memoryStorage()
    const scope = effectScope()
    const draft = scope.run(() => useDraft(() => problem, storage))!
    draft.source.value = 'C draft\n'
    vi.advanceTimersByTime(999)
    expect(storage.getItem('acorn:draft:v1:1:c')).toBeNull()
    vi.advanceTimersByTime(1)
    expect(draft.saved.value).toBe(true)
    draft.source.value = 'updated C\n'
    draft.selectLanguage('python')
    expect(JSON.parse(storage.getItem('acorn:draft:v1:1:c')!).source).toBe(
      'updated C\n',
    )
    expect(draft.source.value).toBe('')
    draft.source.value = 'print(1)\n'
    scope.stop()

    const restoredScope = effectScope()
    const restored = restoredScope.run(() => useDraft(() => problem, storage))!
    expect(restored.languageId.value).toBe('python')
    expect(restored.source.value).toBe('print(1)\n')
    restored.selectLanguage('c')
    expect(restored.source.value).toBe('updated C\n')
    const other = restoredScope.run(() =>
      useDraft(() => ({ ...problem, problem_id: 2 }), storage),
    )!
    expect(other.source.value).toBe('')
    restoredScope.stop()
  })

  it('retains source across version changes, saves the displayed version and notices it only once', () => {
    const storage = memoryStorage()
    storage.setItem(
      'acorn:draft:v1:1:c',
      JSON.stringify({ source: 'C code', version_no: 1, updated_at_ms: 1 }),
    )
    storage.setItem(
      'acorn:draft:v1:1:python',
      JSON.stringify({
        source: 'Python code',
        version_no: 1,
        updated_at_ms: 1,
      }),
    )
    const currentProblem = ref({ ...problem, version_no: 2 })
    const scope = effectScope()
    const draft = scope.run(() =>
      useDraft(() => currentProblem.value, storage),
    )!
    expect(draft.source.value).toBe('C code')
    expect(draft.versionNotice.value).toContain('v2')
    expect(JSON.parse(storage.getItem('acorn:draft:v1:1:c')!).version_no).toBe(
      2,
    )
    draft.dismissVersionNotice()
    draft.selectLanguage('python')
    expect(draft.source.value).toBe('Python code')
    expect(draft.versionNotice.value).toBeNull()
    currentProblem.value = { ...problem, version_no: 3 }
    expect(draft.versionNotice.value).toContain('v3')
    scope.stop()
    const reopenedScope = effectScope()
    const reopened = reopenedScope.run(() =>
      useDraft(() => currentProblem.value, storage),
    )!
    expect(reopened.versionNotice.value).toBeNull()
    reopenedScope.stop()
  })

  it('keeps editable memory and an unsaved status after storage failure without logging source', () => {
    vi.useFakeTimers()
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const storage = memoryStorage()
    const scope = effectScope()
    const draft = scope.run(() => useDraft(() => problem, storage))!
    draft.source.value = 'private-source'
    expect(draft.save()).toBe(true)
    storage.setItem.mockImplementation(() => {
      throw new Error('private-source')
    })
    expect(draft.save()).toBe(false)
    expect(draft.saved.value).toBe(false)
    expect(draft.storageWarning.value).toContain('无法保存')
    draft.source.value = 'changed-private-source'
    draft.selectLanguage('python')
    draft.selectLanguage('c')
    expect(draft.source.value).toBe('changed-private-source')
    scope.stop()
    const unavailableScope = effectScope()
    const unavailable = unavailableScope.run(() =>
      useDraft(() => problem, null),
    )!
    unavailable.source.value = 'still editable'
    expect(unavailable.save()).toBe(false)
    expect(unavailable.saved.value).toBe(false)
    unavailableScope.stop()
    expect(JSON.stringify(warn.mock.calls)).not.toContain('private-source')
  })

  it('clears only the confirmed language and cancels a pending auto-save', () => {
    vi.useFakeTimers()
    const storage = memoryStorage()
    storage.setItem('acorn:draft:v1:1:python', 'other-language')
    storage.setItem('acorn:draft:v1:2:c', 'other-problem')
    storage.setItem('acorn:pending-submission:v1', 'frozen request')
    const scope = effectScope()
    const draft = scope.run(() => useDraft(() => problem, storage))!
    draft.source.value = 'C code'
    draft.save()
    draft.source.value = 'pending code'
    expect(draft.clearConfirmed()).toBe(true)
    vi.advanceTimersByTime(1000)
    scope.stop()
    expect(draft.source.value).toBe('')
    expect(storage.getItem('acorn:draft:v1:1:c')).toBeNull()
    expect(storage.getItem('acorn:draft:v1:1:python')).toBe('other-language')
    expect(storage.getItem('acorn:draft:v1:2:c')).toBe('other-problem')
    expect(storage.getItem('acorn:pending-submission:v1')).toBe(
      'frozen request',
    )
  })
})
