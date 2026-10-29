import { computed, readonly, ref, shallowRef, watch } from 'vue'
import {
  getMe,
  login as loginApi,
  logout as logoutApi,
  register as registerApi,
} from '../api'
import { RequestError } from '../api/http'
import type { LoginRequest, RegisterRequest, User } from '../api/types'

/** Page identity captured before an operation, independent of a later shared Cookie. */
export interface Identity {
  readonly userId: number
  readonly generation: number
}

/** One application owns authentication state; tests create independent instances. */
export function createAuthState() {
  const user = shallowRef<User | null>(null)
  const generation = ref(0)
  const status = ref<'unknown' | 'loading' | 'ready' | 'error'>('unknown')
  const loadError = shallowRef<unknown>(null)
  const formError = shallowRef<unknown>(null)
  const busy = ref(false)
  const dialogOpen = ref(false)
  let loadController: AbortController | undefined
  let loadSequence = 0
  let loading: Promise<void> | undefined
  let operation = 0
  let operationController: AbortController | undefined

  watch(
    generation,
    () => {
      operation += 1
      operationController?.abort()
      operationController = undefined
      busy.value = false
    },
    { flush: 'sync' },
  )

  function snapshot(): Identity | null {
    return status.value === 'ready' && user.value
      ? { userId: user.value.id, generation: generation.value }
      : null
  }

  function isCurrent(identity: Identity) {
    return (
      status.value === 'ready' &&
      user.value?.id === identity.userId &&
      generation.value === identity.generation
    )
  }

  function adopt(next: User | null) {
    loadController?.abort()
    loadSequence += 1
    loading = undefined
    generation.value += 1
    status.value = 'ready'
    user.value = next
    loadError.value = null
  }

  /** Invalidate private pages synchronously, then query the Cookie's actual identity. */
  async function refresh() {
    loadController?.abort()
    const sequence = ++loadSequence
    status.value = 'loading'
    generation.value += 1
    user.value = null
    loadError.value = null
    const controller = new AbortController()
    loadController = controller
    const task = (async () => {
      try {
        const result = await getMe({ signal: controller.signal })
        if (sequence !== loadSequence) return
        status.value = 'ready'
        user.value = result.user
      } catch (cause) {
        if (sequence !== loadSequence || controller.signal.aborted) return
        loadError.value = cause
        status.value = 'error'
      }
    })()
    loading = task
    await task
    if (loading === task) loading = undefined
  }

  function initialize() {
    if (loading) return loading
    return status.value === 'unknown' ? refresh() : Promise.resolve()
  }

  function openLogin() {
    formError.value = null
    dialogOpen.value = true
  }

  function clearFormError() {
    formError.value = null
  }

  function closeDialog() {
    if (!busy.value) dialogOpen.value = false
  }

  function requireLogin() {
    if (snapshot()) return true
    openLogin()
    return false
  }

  /** Only business authentication failures invalidate an already confirmed identity. */
  function handleError(cause: unknown, identity: Identity) {
    if (
      !(cause instanceof RequestError) ||
      !['AuthenticationRequired', 'SessionChanged'].includes(cause.code ?? '')
    )
      return false
    if (!isCurrent(identity)) return true
    if (cause.code === 'SessionChanged') {
      formError.value = cause
      void refresh().then(() => {
        if (status.value === 'ready' && !user.value) openLogin()
      })
    } else {
      adopt(null)
      openLogin()
    }
    return true
  }

  async function login(request: LoginRequest) {
    if (busy.value) return false
    const current = ++operation
    const identityGeneration = generation.value
    const controller = new AbortController()
    operationController = controller
    busy.value = true
    formError.value = null
    try {
      const result = await loginApi(request, { signal: controller.signal })
      if (current !== operation || identityGeneration !== generation.value)
        return false
      adopt(result.user)
      dialogOpen.value = false
      return true
    } catch (cause) {
      if (current === operation && identityGeneration === generation.value)
        formError.value = cause
      return false
    } finally {
      if (current === operation) {
        busy.value = false
        operationController = undefined
      }
    }
  }

  async function register(request: RegisterRequest) {
    if (busy.value) return false
    const current = ++operation
    const identityGeneration = generation.value
    const controller = new AbortController()
    operationController = controller
    busy.value = true
    formError.value = null
    try {
      await registerApi(request, { signal: controller.signal })
      return current === operation && identityGeneration === generation.value
    } catch (cause) {
      if (current === operation && identityGeneration === generation.value)
        formError.value = cause
      return false
    } finally {
      if (current === operation) {
        busy.value = false
        operationController = undefined
      }
    }
  }

  async function logout() {
    const identity = snapshot()
    if (!identity || busy.value) return false
    const current = ++operation
    const controller = new AbortController()
    operationController = controller
    busy.value = true
    formError.value = null
    try {
      await logoutApi({
        expectedUserId: identity.userId,
        signal: controller.signal,
      })
      if (!isCurrent(identity)) return false
      adopt(null)
      return true
    } catch (cause) {
      if (isCurrent(identity) && !handleError(cause, identity))
        formError.value = cause
      return false
    } finally {
      if (current === operation) {
        busy.value = false
        operationController = undefined
      }
    }
  }

  return {
    user: readonly(user),
    generation: readonly(generation),
    status: readonly(status),
    loadError: readonly(loadError),
    formError: readonly(formError),
    busy: readonly(busy),
    dialogOpen: readonly(dialogOpen),
    authenticated: computed(() => snapshot() !== null),
    snapshot,
    isCurrent,
    initialize,
    refresh,
    openLogin,
    closeDialog,
    clearFormError,
    requireLogin,
    handleError,
    login,
    register,
    logout,
  }
}

const auth = createAuthState()

export function useAuth() {
  return auth
}
