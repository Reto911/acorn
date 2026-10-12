import { log } from '../utils/log'
import type { HttpErrorBody } from './types'

export type RequestErrorKind =
  'http' | 'network' | 'timeout' | 'cancelled' | 'invalid-response'

export interface RequestContext {
  operation: string
  path: string
  method: 'GET' | 'POST'
}

/** HTTP operation failure, never a persisted judging verdict or execution failure. */
export class RequestError extends Error {
  constructor(
    public readonly kind: RequestErrorKind,
    public readonly context: RequestContext,
    public readonly status?: number,
    public readonly body?: HttpErrorBody,
  ) {
    super(`Request failed: ${kind}`)
    this.name = 'RequestError'
  }

  get code() {
    return this.body?.code
  }

  get submissionId() {
    return this.body?.submission_id
  }
}

export interface HttpResponse<T> {
  data: T
  status: number
  duration_ms: number
}

interface RequestOptions {
  operation: string
  method?: 'GET' | 'POST'
  body?: unknown
  idempotencyKey?: string
  signal?: AbortSignal
  /** Creation must validate its acknowledgement before clearing the saved request. */
  validate?: (value: unknown, status: number) => boolean
}

function errorBody(value: unknown): HttpErrorBody | undefined {
  if (typeof value !== 'object' || value === null || !('error' in value)) return
  const error = value.error
  if (
    typeof error !== 'object' ||
    error === null ||
    !('code' in error) ||
    typeof error.code !== 'string' ||
    !('message' in error) ||
    typeof error.message !== 'string'
  )
    return
  return {
    code: error.code,
    message: error.message,
    ...('submission_id' in error && typeof error.submission_id === 'string'
      ? { submission_id: error.submission_id }
      : {}),
  }
}

/** One attempt only. Aborting browser waiting does not cancel backend creation. */
export async function requestJson<T>(
  path: string,
  options: RequestOptions,
): Promise<HttpResponse<T>> {
  const context: RequestContext = {
    operation: options.operation,
    path,
    method: options.method ?? 'GET',
  }
  const startedAt = performance.now()
  const controller = new AbortController()
  let abortKind: 'timeout' | 'cancelled' = 'cancelled'
  const abort = (kind: typeof abortKind) => {
    if (controller.signal.aborted) return
    abortKind = kind
    controller.abort()
  }
  const cancel = () => abort('cancelled')
  options.signal?.addEventListener('abort', cancel, { once: true })
  const timer = setTimeout(() => abort('timeout'), 10_000)
  let status: number | undefined

  try {
    if (options.signal?.aborted) cancel()
    controller.signal.throwIfAborted()
    const headers: Record<string, string> = { Accept: 'application/json' }
    if (options.body !== undefined) headers['Content-Type'] = 'application/json'
    if (options.idempotencyKey)
      headers['Idempotency-Key'] = options.idempotencyKey
    const response = await fetch(path, {
      method: context.method,
      headers,
      body:
        options.body === undefined ? undefined : JSON.stringify(options.body),
      signal: controller.signal,
    })
    status = response.status
    const text = await response.text()
    let data: unknown
    try {
      data = JSON.parse(text)
    } catch {
      // JSON parse errors can echo response bytes; do not retain or log them.
      throw new RequestError('invalid-response', context, status)
    }
    if (!response.ok)
      throw new RequestError('http', context, status, errorBody(data))
    if (options.validate && !options.validate(data, status))
      throw new RequestError('invalid-response', context, status)
    return {
      data: data as T,
      status,
      duration_ms: Math.round(performance.now() - startedAt),
    }
  } catch (cause) {
    const error = controller.signal.aborted
      ? new RequestError(abortKind, context, status)
      : cause instanceof RequestError
        ? cause
        : new RequestError('network', context, status)
    log(
      error.kind === 'cancelled'
        ? 'debug'
        : error.kind === 'http' &&
            (error.status! < 500 ||
              error.code === 'QueueFull' ||
              error.code === 'BackendStopping')
          ? 'warn'
          : 'error',
      {
        ...context,
        status: error.status,
        code: error.code,
        submission_id: error.submissionId,
        reason: error.kind,
        duration_ms: Math.round(performance.now() - startedAt),
      },
    )
    throw error
  } finally {
    clearTimeout(timer)
    options.signal?.removeEventListener('abort', cancel)
  }
}
