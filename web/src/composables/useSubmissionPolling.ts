import { computed, onScopeDispose, shallowRef, watch } from 'vue'
import type { ReviewResponse, Submission } from '../api/types'
import { logSubmissionState } from '../utils/log'

type PageVisibility = Pick<
  Document,
  'hidden' | 'addEventListener' | 'removeEventListener'
>

/** Poll one submission serially, retaining snapshots on errors until manual retry. */
export function useSubmissionPolling(
  submissionId: () => string,
  load: (id: string, signal: AbortSignal) => Promise<Submission>,
  visibility: PageVisibility = document,
) {
  const data = shallowRef<Submission | null>(null)
  const error = shallowRef<unknown>(null)
  const loading = shallowRef(false)
  const hidden = shallowRef(visibility.hidden)
  const refreshRequired = shallowRef(false)
  const terminal = computed(
    () =>
      (data.value?.state === 'finished' || data.value?.state === 'failed') &&
      data.value.review_status !== 'PENDING' &&
      !refreshRequired.value,
  )
  let controller: AbortController | undefined
  let timer: ReturnType<typeof setTimeout> | undefined
  let generation = 0
  let disposed = false

  function stop() {
    generation += 1
    clearTimeout(timer)
    timer = undefined
    controller?.abort()
    controller = undefined
    loading.value = false
  }

  async function refresh() {
    if (disposed || hidden.value || loading.value) return
    clearTimeout(timer)
    timer = undefined
    const current = ++generation
    controller = new AbortController()
    const signal = controller.signal
    loading.value = true
    error.value = null
    try {
      const snapshot = await load(submissionId(), signal)
      if (current !== generation) return
      logSubmissionState(
        snapshot.submission_id,
        snapshot.state,
        data.value?.state,
      )
      data.value = snapshot
      refreshRequired.value = false
    } catch (cause) {
      if (current === generation && !signal.aborted) error.value = cause
    } finally {
      if (current === generation) {
        controller = undefined
        loading.value = false
        if (!error.value && !terminal.value && !hidden.value)
          timer = setTimeout(() => void refresh(), 1000)
      }
    }
  }

  /** Discard any older GET, apply an accepted review snapshot and immediately query again. */
  async function reconcileReview(response?: ReviewResponse) {
    if (disposed) return
    stop()
    refreshRequired.value = true
    if (response && data.value?.submission_id === response.submission_id)
      data.value = {
        ...data.value,
        review_status: response.review_status,
        review: response.review,
      }
    await refresh()
  }

  function visibilityChanged() {
    hidden.value = visibility.hidden
    if (hidden.value) stop()
    else if (!terminal.value && !error.value) void refresh()
  }

  visibility.addEventListener('visibilitychange', visibilityChanged)
  watch(
    submissionId,
    () => {
      stop()
      data.value = null
      error.value = null
      refreshRequired.value = false
      void refresh()
    },
    { immediate: true },
  )
  onScopeDispose(() => {
    disposed = true
    stop()
    visibility.removeEventListener('visibilitychange', visibilityChanged)
  })

  return {
    data,
    error,
    loading,
    hidden,
    terminal,
    refreshRequired,
    refresh,
    reconcileReview,
  }
}
