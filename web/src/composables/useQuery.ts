import { onScopeDispose, shallowRef, watch } from 'vue'
import { RequestError } from '../api/http'

/** Page identity/query changes cancel old requests and invalidate late responses. */
export function useQuery<Q, T>(
  source: () => Q,
  load: (query: Q, signal: AbortSignal) => Promise<T>,
) {
  const data = shallowRef<T | null>(null)
  const error = shallowRef<unknown>(null)
  const loading = shallowRef(false)
  let controller: AbortController | undefined
  let generation = 0

  async function execute(preserveData: boolean) {
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
      )
        error.value = cause
    } finally {
      if (current === generation) loading.value = false
    }
  }

  watch(source, () => void execute(false), { immediate: true, deep: true })
  onScopeDispose(() => {
    generation += 1
    controller?.abort()
  })

  return { data, error, loading, reload: () => execute(true) }
}
