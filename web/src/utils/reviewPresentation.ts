import type {
  FindingCategory,
  ReviewErrorCode,
  ReviewStatus,
} from '../api/types'

/** Result pages and history use the same labels, independently of machine verdicts. */
export const reviewStatuses: Record<ReviewStatus, string> = {
  DISABLED: '未启用／未触发',
  PENDING: '审查中',
  PASS: '通过',
  DOUBTFUL: '存疑',
  FAILED: '审查失败',
}

export const findingCategories: Record<FindingCategory, string> = {
  TEST_TARGETING: '针对测试数据',
  JUDGE_ENVIRONMENT_ABUSE: '滥用判题环境',
  STATEMENT_CONSTRAINT_VIOLATION: '违反题面约束',
}

/** Explain stable backend failures without treating provider text as a review opinion. */
export const reviewErrors: Record<ReviewErrorCode, string> = {
  ReviewUnavailable: '审查服务未启用或暂不可用。',
  ReviewQueueFull: '审查队列已满，本次未能排入队列。',
  ReviewInputUnavailable: '未能取得此次提交的源码或固定版本题面。',
  ReviewInputTooLarge: '完整审查输入超过字节上限。',
  ReviewTimeout: '等待模型响应超时。',
  ReviewApiFailed: '模型接口调用失败。',
  ReviewIncomplete: '模型未完成本次响应。',
  ReviewInvalidOutput: '模型响应未满足审查格式或内容预算。',
  ReviewInterrupted: '后端停止或重启中断了本次审查。',
}
