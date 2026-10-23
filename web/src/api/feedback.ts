import { RequestError } from './http'

export interface ErrorFeedback {
  title: string
  description: string
  code?: string
  detail?: string
}

const messages: Record<string, [string, string]> = {
  InvalidRequest: ['请求内容有误', '请检查题号、语言、源码或查询参数后重试。'],
  InvalidIdempotencyKey: [
    '提交请求标识无效',
    '本次请求已结束，请重新主动提交；若持续出现，请联系维护者。',
  ],
  UnsupportedMediaType: ['请求格式不受支持', '请刷新页面后重试或联系维护者。'],
  RequestTooLarge: ['请求内容过大', '请缩减源码后重新提交。'],
  SourceTooLarge: ['源码超过字节上限', '请缩减源码后重新提交。'],
  UnsupportedLanguage: ['不支持所选语言', '请选择 C 或 Python 3 后重新提交。'],
  LanguageNotAllowed: [
    '本题不允许所选语言',
    '请刷新题面，选择当前允许的语言后重新提交。',
  ],
  ProblemNotFound: ['题目不存在', '该题可能尚未发布，请返回题目列表。'],
  ProblemVersionNotFound: [
    '指定版本不存在',
    '未能取得这次提交对应的题面；已取得的提交结果仍可查看。',
  ],
  SubmissionNotFound: ['提交不存在', '请核对提交链接，或从提交历史重新打开。'],
  IdempotencyConflict: [
    '提交请求标识与内容冲突',
    '本次请求已结束，请先查看提交历史；再次主动提交可能创建另一条记录。',
  ],
  QueueFull: ['当前排队任务较多', '原提交请求已保留，请稍后手动重试原提交。'],
  WorkersUnavailable: [
    '暂未观测到可用 Worker',
    '此次未创建提交，原请求已保留，请稍后手动重试。',
  ],
  JudgingNotFinished: [
    '机器判题尚未完成',
    '请刷新提交状态，只有机器判题已完成的提交可以启动审查。',
  ],
  ReviewChanged: [
    '最新审查已变化',
    '页面会查询最新状态；请查看最新结果后再决定是否重新审查，不会自动再次发送。',
  ],
  ReviewUnavailable: [
    '审查服务暂不可用',
    '此次审查未接纳，原审查结果保留，请稍后手动操作。',
  ],
  ReviewQueueFull: [
    '审查队列已满',
    '此次审查未接纳，原审查结果保留，请稍后手动操作。',
  ],
  BackendStopping: [
    '服务暂不接纳新提交',
    '原提交请求已保留，请稍后手动重试；已有提交仍可查询。',
  ],
  RouteNotFound: ['接口不存在', '请检查前端与后端部署是否匹配，或联系维护者。'],
  MethodNotAllowed: ['接口不支持当前操作', '请刷新页面后重试或联系维护者。'],
  InternalError: [
    '服务未能完成本次请求',
    '请稍后重试；本次请求失败不代表提交判题失败。',
  ],
}

/** UI guidance describes this operation, without claiming drafts were saved. */
export function errorFeedback(
  error: unknown,
  operation: 'query' | 'create' | 'review' = 'query',
): ErrorFeedback {
  if (operation === 'review' && reviewNeedsRefresh(error))
    return error instanceof RequestError && error.code === 'ReviewChanged'
      ? {
          title: messages.ReviewChanged![0],
          description: messages.ReviewChanged![1],
          code: error.code,
        }
      : {
          title: '尚未确认审查请求是否接纳',
          description:
            '请以最新查询状态为准，再决定是否手动操作；不会自动再次发送审查请求，停止等待也不会撤销后端已登记的操作。',
          code: error instanceof RequestError ? error.code : undefined,
          detail:
            error instanceof RequestError ? error.body?.message : undefined,
        }
  if (!(error instanceof RequestError))
    return {
      title: '操作未能完成',
      description: '请重试；若持续出现，请联系维护者。',
    }
  if (operation === 'review' && error.code === 'BackendStopping')
    return {
      title: '服务已停止接纳审查',
      description: '此次审查未接纳，原审查结果保留，请稍后手动操作。',
      code: error.code,
    }
  const createUnknown =
    operation === 'create' &&
    (error.kind !== 'http' ||
      (error.status !== undefined &&
        error.status >= 500 &&
        error.code !== 'QueueFull' &&
        error.code !== 'BackendStopping' &&
        error.code !== 'WorkersUnavailable'))
  if (createUnknown)
    return {
      title: '尚未确认是否提交成功',
      description:
        '原请求已保留，请重试原提交或查看提交历史；超时或停止等待不会撤销后端提交。',
      code: error.code,
      detail: error.body?.message,
    }
  if (error.code === 'QueueFull' && error.submissionId)
    return {
      title: '提交已保存，尚未完成排队交接',
      description:
        '原请求已保留，可查看原记录或稍后手动重试原提交；查看记录不会重新交接任务。',
      code: error.code,
      detail: error.body?.message,
    }
  const message =
    error.code && Object.hasOwn(messages, error.code)
      ? messages[error.code]
      : undefined
  if (message)
    return {
      title: message[0],
      description: message[1],
      code: error.code,
      detail: error.body?.message,
    }
  const title = {
    timeout: '请求等待超时',
    network: '未能连接服务',
    cancelled: '已停止等待本次请求',
    'invalid-response': '服务响应无法读取',
    http: '服务未能完成本次请求',
  }[error.kind]
  return {
    title,
    description:
      operation === 'create'
        ? '原请求已保留，请稍后手动重试原提交或查看提交历史。'
        : '本次查询未成功，请手动重试；已有结果可能过期，不能据此判断判题失败。',
    code: error.code,
    detail: error.body?.message,
  }
}

/** Uncertain POST outcomes and stale identities require GET before another manual attempt. */
export function reviewNeedsRefresh(error: unknown) {
  return (
    !(error instanceof RequestError) ||
    error.code === 'ReviewChanged' ||
    error.kind !== 'http' ||
    ((error.status ?? 0) >= 500 &&
      !['ReviewUnavailable', 'ReviewQueueFull', 'BackendStopping'].includes(
        error.code ?? '',
      ))
  )
}

/** Only documented, definitive rejections end a create attempt automatically. */
export function endsCreation(error: RequestError) {
  return (
    error.kind === 'http' &&
    error.status !== undefined &&
    error.status < 500 &&
    error.code !== undefined &&
    Object.hasOwn(messages, error.code) &&
    error.code !== 'QueueFull' &&
    error.code !== 'BackendStopping' &&
    error.code !== 'InternalError'
  )
}
