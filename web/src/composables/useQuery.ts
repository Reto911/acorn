import { onScopeDispose, shallowRef, watch } from 'vue'
import { RequestError } from '../api/http'

/** Cancel obsolete queries; optional periodic refreshes preserve data and skip busy ticks. */
export function useQuery<Q, T>(
  source: () => Q,
  load: (query: Q, signal: AbortSignal) => Promise<T>,
  options: {
    refreshIntervalMs?: number
    handleError?: (error: unknown) => boolean
  } = {},
) {
  const data = shallowRef<T | null>(null)
  const error = shallowRef<unknown>(null)
  const loading = shallowRef(false)
  let controller: AbortController | undefined
  let generation = 0
  let stopped = false

  /** Stop this identity's requests and discard its private snapshots immediately. */
  function stop() {
    stopped = true
    generation += 1
    clearInterval(refreshTimer)
    controller?.abort()
    controller = undefined
    data.value = null
    error.value = null
    loading.value = false
  }

  async function execute(preserveData: boolean) {
    if (stopped) return
    const current = ++generation
    controller?.abort()
    controller = new AbortController()
    const signal = controller.signal
    if (!preserveData) data.value = null
    error.value = null
    loading.value = true
    try {
      const result = await load(source(), signal)
      if (current === generation) data.value = result
    } catch (cause) {
      if (
        current === generation &&
        !signal.aborted &&
        !(cause instanceof RequestError && cause.kind === 'cancelled')
      ) {
        if (options.handleError?.(cause)) stop()
        else error.value = cause
      }
    } finally {
      if (current === generation) loading.value = false
    }
  }

  const reload = () => execute(true)
  const refreshTimer =
    options.refreshIntervalMs === undefined
      ? undefined
      : setInterval(() => {
          if (!stopped && !loading.value) void reload()
        }, options.refreshIntervalMs)
  watch(source, () => void execute(false), { immediate: true, deep: true })
  onScopeDispose(stop)

  return { data, error, loading, reload, stop }
}
