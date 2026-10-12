import { createApp } from 'vue'
import { afterEach, expect, it, vi } from 'vitest'
import {
  dismissRuntimeError,
  installErrorHandlers,
  runtimeErrorVisible,
} from './browserErrors'
import { logSubmissionState } from './log'

afterEach(() => {
  dismissRuntimeError()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

it('logs Vue/browser/rejection exceptions once, hides sensitive messages and shows actionable UI', () => {
  const target = new EventTarget()
  vi.stubGlobal('window', target)
  const print = vi.spyOn(console, 'error').mockImplementation(() => {})
  const app = createApp({})
  installErrorHandlers(app)
  const privateError = new Error('private source, body, key and program output')
  app.config.errorHandler!(privateError, null, 'component update')
  const browserEvent = new Event('error', { cancelable: true })
  Object.assign(browserEvent, { error: privateError })
  target.dispatchEvent(browserEvent)
  expect(print).toHaveBeenCalledOnce()
  expect(browserEvent.defaultPrevented).toBe(true)
  const rejection = new Event('unhandledrejection', { cancelable: true })
  Object.assign(rejection, { reason: 'private rejection text' })
  target.dispatchEvent(rejection)
  expect(print).toHaveBeenCalledTimes(2)
  expect(rejection.defaultPrevented).toBe(true)
  expect(runtimeErrorVisible.value).toBe(true)
  const logs = JSON.stringify(print.mock.calls)
  expect(logs).not.toContain('private source')
  expect(logs).not.toContain('private rejection')
  expect(logs).toContain('stack')
  dismissRuntimeError()
  expect(runtimeErrorVisible.value).toBe(false)
})

it('logs only business state changes at info', () => {
  const info = vi.spyOn(console, 'info').mockImplementation(() => {})
  logSubmissionState('submission-id', 'running', 'running')
  expect(info).not.toHaveBeenCalled()
  logSubmissionState('submission-id', 'finished', 'running')
  expect(info).toHaveBeenCalledOnce()
  expect(info).toHaveBeenCalledWith(
    '[Acorn]',
    expect.objectContaining({
      state: 'finished',
      previous_state: 'running',
      submission_id: 'submission-id',
    }),
  )
})
