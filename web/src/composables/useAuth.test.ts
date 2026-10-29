import { effectScope, type EffectScope } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getMe, login, logout, register } from '../api'
import { RequestError } from '../api/http'
import type { AuthResponse } from '../api/types'
import { createAuthState } from './useAuth'

vi.mock('../api', () => ({
  getMe: vi.fn(),
  login: vi.fn(),
  logout: vi.fn(),
  register: vi.fn(),
}))
const first = { id: 1, username: 'first', display_name: 'First' }
const second = { id: 2, username: 'second', display_name: 'Second' }
const credentials = { username: 'first', password: 'password' }
const scopes: EffectScope[] = []

function state() {
  const scope = effectScope()
  scopes.push(scope)
  return scope.run(createAuthState)!
}

function deferred() {
  let resolve!: (response: AuthResponse) => void
  const promise = new Promise<AuthResponse>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function rejection(code: string, status: number) {
  return new RequestError(
    'http',
    { method: 'POST', operation: 'test', path: '/api/v1/submissions' },
    status,
    { code, message: 'Rejected' },
  )
}

beforeEach(() => {
  vi.mocked(getMe).mockResolvedValue({ user: null })
  vi.mocked(login).mockResolvedValue({ user: first })
  vi.mocked(register).mockResolvedValue({ user: first })
  vi.mocked(logout).mockResolvedValue({ user: null })
})

afterEach(() => {
  for (const scope of scopes.splice(0)) scope.stop()
  vi.resetAllMocks()
})

describe('shared browser identity', () => {
  it('keeps unknown/loading/error separate from confirmed anonymous and allows retry', async () => {
    const auth = state()
    expect(auth.status.value).toBe('unknown')
    expect(auth.snapshot()).toBeNull()
    const waiting = deferred()
    vi.mocked(getMe).mockReturnValueOnce(waiting.promise)
    const initial = auth.initialize()
    const alsoInitial = auth.initialize()
    expect(getMe).toHaveBeenCalledOnce()
    expect(auth.status.value).toBe('loading')
    waiting.resolve({ user: null })
    await Promise.all([initial, alsoInitial])
    expect(auth.status.value).toBe('ready')
    vi.mocked(getMe).mockRejectedValueOnce(new Error('Unavailable'))
    await auth.refresh()
    expect(auth.status.value).toBe('error')
    expect(auth.loadError.value).toBeInstanceOf(Error)
    expect(auth.snapshot()).toBeNull()
    vi.mocked(getMe).mockResolvedValueOnce({ user: first })
    await auth.refresh()
    expect(auth.snapshot()?.userId).toBe(1)
    expect(auth.loadError.value).toBeNull()
  })

  it('registers without login, keeps an existing identity after wrong credentials, and logs out with its owner', async () => {
    const auth = state()
    await auth.initialize()
    auth.openLogin()
    expect(await auth.register(credentials)).toBe(true)
    expect(auth.user.value).toBeNull()
    expect(auth.dialogOpen.value).toBe(true)
    expect(login).not.toHaveBeenCalled()
    expect(await auth.login(credentials)).toBe(true)
    const identity = auth.snapshot()!
    expect(auth.dialogOpen.value).toBe(false)
    auth.openLogin()
    vi.mocked(login).mockRejectedValueOnce(rejection('InvalidCredentials', 401))
    expect(await auth.login(credentials)).toBe(false)
    expect(auth.isCurrent(identity)).toBe(true)
    expect(auth.formError.value).toMatchObject({ code: 'InvalidCredentials' })
    vi.mocked(logout).mockRejectedValueOnce(new Error('Network failure'))
    expect(await auth.logout()).toBe(false)
    expect(auth.isCurrent(identity)).toBe(true)
    expect(await auth.logout()).toBe(true)
    expect(logout).toHaveBeenLastCalledWith({
      expectedUserId: 1,
      signal: expect.any(AbortSignal),
    })
    expect(auth.user.value).toBeNull()
    expect(auth.isCurrent(identity)).toBe(false)
  })

  it('handles only authentication business errors and confirms the new Cookie on SessionChanged', async () => {
    const auth = state()
    await auth.initialize()
    await auth.login(credentials)
    const identity = auth.snapshot()!
    for (const [code, status] of [
      ['InvalidCredentials', 401],
      ['UsernameTaken', 409],
      ['ReviewChanged', 409],
      ['IdempotencyConflict', 409],
    ] as const)
      expect(auth.handleError(rejection(code, status), identity)).toBe(false)
    const changed = deferred()
    vi.mocked(getMe).mockReturnValueOnce(changed.promise)
    expect(auth.handleError(rejection('SessionChanged', 409), identity)).toBe(
      true,
    )
    expect(auth.user.value).toBeNull()
    expect(auth.status.value).toBe('loading')
    changed.resolve({ user: second })
    await auth.initialize()
    expect(auth.user.value).toEqual(second)
    expect(auth.dialogOpen.value).toBe(false)
    expect(logout).not.toHaveBeenCalled()
    expect(
      auth.handleError(rejection('AuthenticationRequired', 401), identity),
    ).toBe(true)
    expect(auth.user.value).toEqual(second)
    const current = auth.snapshot()!
    auth.handleError(rejection('AuthenticationRequired', 401), current)
    expect(auth.user.value).toBeNull()
    expect(auth.dialogOpen.value).toBe(true)
  })

  it('ignores a late me result after login and does not keep navigation waiting for that obsolete request', async () => {
    const auth = state()
    const waiting = deferred()
    vi.mocked(getMe).mockReturnValueOnce(waiting.promise)
    const initial = auth.initialize()
    const signal = vi.mocked(getMe).mock.calls[0]![0]!.signal!
    await auth.login(credentials)
    expect(signal.aborted).toBe(true)
    await auth.initialize()
    waiting.resolve({ user: second })
    await initial
    expect(auth.user.value).toEqual(first)
    expect(auth.status.value).toBe('ready')
  })

  it('cancels and ignores late authentication results after an identity generation changes', async () => {
    const auth = state()
    await auth.initialize()
    await auth.login(credentials)
    const old = auth.snapshot()!
    const pending = deferred()
    vi.mocked(login).mockReturnValueOnce(pending.promise)
    const attempt = auth.login({ ...credentials, username: 'second' })
    const signal = vi.mocked(login).mock.calls.at(-1)![1]!.signal!
    auth.handleError(rejection('AuthenticationRequired', 401), old)
    expect(signal.aborted).toBe(true)
    expect(auth.busy.value).toBe(false)
    pending.resolve({ user: second })
    expect(await attempt).toBe(false)
    expect(auth.user.value).toBeNull()
    expect(auth.formError.value).toBeNull()
  })
})
