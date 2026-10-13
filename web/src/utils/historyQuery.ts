import type { LocationQuery } from 'vue-router'
import type {
  HistoryQuery,
  ReviewStatus,
  SubmissionState,
  Verdict,
} from '../api/types'

/** Read only history fields; invalid URL values remain subject to API validation. */
export function readHistoryQuery(query: LocationQuery): HistoryQuery {
  const value = (key: string) =>
    query[key] == null || query[key] === '' ? undefined : String(query[key])
  const number = (key: string) => {
    const text = value(key)
    return text === undefined ? undefined : Number(text)
  }
  return {
    limit: number('limit'),
    offset: number('offset'),
    problem_id: number('problem_id'),
    state: value('state') as SubmissionState | undefined,
    verdict: value('verdict') as Verdict | undefined,
    review_status: value('review_status') as ReviewStatus | undefined,
  }
}
