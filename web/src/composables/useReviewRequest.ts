import { computed, onScopeDispose, shallowRef, watch } from 'vue'
import { startReview } from '../api'
import { reviewNeedsRefresh } from '../api/feedback'
import type { ReviewResponse, Submission } from '../api/types'

/** Own only the browser wait; the backend's registered admission survives cancellation. */
export function useReviewRequest(
  submission: () => Submission | null,
  refreshRequired: () => boolean,
  reconcile: (response?: ReviewResponse) => Promise<void>,
  send = startReview,
  options: {
    expectedUserId?: number
    handleError?: (error: unknown) => boolean
  } = {},
) {
  const sending = shallowRef(false)
  const error = shallowRef<unknown>(null)
  const stopped = shallowRef(false)
  const canStart = computed(
    () =>
      submission()?.state === 'finished' &&
      submission()?.review_status !== 'PENDING' &&
      !stopped.value &&
      !sending.value &&
      !refreshRequired(),
  )
  let controller: AbortController | undefined
  let generation = 0

  function cancel() {
    generation += 1
    controller?.abort()
    controller = undefined
    sending.value = false
    error.value = null
  }

  /** Invalidate this identity's browser wait without retrying its admitted operation. */
  function stop() {
    stopped.value = true
    cancel()
  }

  async function start() {
    const snapshot = submission()
    if (!canStart.value || !snapshot) return
    const current = ++generation
    controller = new AbortController()
    sending.value = true
    error.value = null
    try {
      const accepted = await send(
        snapshot.submission_id,
        snapshot.review?.review_id ?? null,
        {
          signal: controller.signal,
          ...(options.expectedUserId === undefined
            ? {}
            : { expectedUserId: options.expectedUserId }),
        },
      )
      if (current !== generation) return
      await reconcile(accepted)
    } catch (cause) {
      if (current !== generation) return
      if (options.handleError?.(cause)) {
        stop()
        return
      }
      error.value = cause
      if (reviewNeedsRefresh(cause)) await reconcile()
    } finally {
      if (current === generation) {
        controller = undefined
        sending.value = false
      }
    }
  }

  watch(() => submission()?.submission_id, cancel, { flush: 'sync' })
  onScopeDispose(stop)
  return { sending, error, canStart, start, stop }
}
