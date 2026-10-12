import { readonly, ref } from 'vue'
import type { App } from 'vue'
import { exceptionContext, log } from './log'

const runtimeError = ref(false)
export const runtimeErrorVisible = readonly(runtimeError)

export function dismissRuntimeError() {
  runtimeError.value = false
}

/** Each unhandled error has one logging entry; payload-like messages stay private. */
export function installErrorHandlers(app: App) {
  const recorded = new WeakSet<object>()
  const report = (reason: unknown, operation: string) => {
    if (typeof reason === 'object' && reason !== null) {
      if (recorded.has(reason)) return
      recorded.add(reason)
    }
    runtimeError.value = true
    log('error', { operation, ...exceptionContext(reason) })
  }
  app.config.errorHandler = (error) => report(error, 'vue-error')
  window.addEventListener('error', (event) => {
    report(event.error, 'browser-error')
    event.preventDefault()
  })
  window.addEventListener('unhandledrejection', (event) => {
    report(event.reason, 'unhandled-rejection')
    event.preventDefault()
  })
}
