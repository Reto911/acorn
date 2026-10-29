import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { describe, expect, it, vi } from 'vitest'
import type { Review, ReviewStatus, Submission } from '../api/types'
import SubmissionReview from './SubmissionReview.vue'

// Node SSR verifies actual escaped markup without loading browser-only style entry points.
vi.mock('element-plus/es/components/button/style/css', () => ({}))
vi.mock('element-plus/es/components/alert/style/css', () => ({}))
vi.mock('element-plus/es/components/empty/style/css', () => ({}))

function renderReview(reviewStatus: ReviewStatus, review: Review | null) {
  const submission: Submission = {
    submission_id: 'submission-a',
    problem_id: 1,
    version_no: 1,
    language_id: 'c',
    state: 'finished',
    verdict: 'AC',
    created_at_ms: 1000,
    terminal_at_ms: 2000,
    terminal_details: null,
    review_status: reviewStatus,
    review,
  }
  return renderToString(
    createSSRApp(SubmissionReview, {
      submission,
      sending: false,
      canStart: true,
      refreshRequired: false,
      error: null,
    }),
  )
}

describe('persisted review presentation', () => {
  it('escapes model text, shows single and inclusive multiple line buttons and keeps doubtful distinct from failed', async () => {
    const html = await renderReview('DOUBTFUL', {
      review_id: 'review-a',
      trigger: 'auto',
      model: '<model>',
      requested_at_ms: 2000,
      completed_at_ms: 3000,
      error: null,
      result: {
        decision: 'DOUBTFUL',
        summary: '<script>alert(1)</script> **原文**',
        findings: [
          {
            category: 'TEST_TARGETING',
            line_start: 2,
            line_end: 4,
            evidence: '<img src=x onerror=alert(1)>',
            normal_explanation: '[说明](javascript:alert(1))',
          },
          {
            category: 'STATEMENT_CONSTRAINT_VIOLATION',
            line_start: 6,
            line_end: 6,
            evidence: '单行证据',
            normal_explanation: '单行解释',
          },
        ],
      },
    })
    expect(html).toContain('存疑')
    const buttons = html.match(/<button\b[^>]*>[\s\S]*?<\/button>/g) ?? []
    expect(buttons.some((button) => button.includes('查看第 2–4 行'))).toBe(
      true,
    )
    expect(buttons.some((button) => button.includes('查看第 6 行'))).toBe(true)
    expect(html).not.toContain('查看第 6–6 行')
    expect(html).toContain('针对测试数据')
    expect(html).toContain('&lt;script&gt;alert(1)&lt;/script&gt; **原文**')
    expect(html).toContain('&lt;img src=x onerror=alert(1)&gt;')
    expect(html).toContain('&lt;model&gt;')
    expect(html).toContain('[说明](javascript:alert(1))')
    expect(html).not.toMatch(/<script>|<img|<a |<strong>|失败代码/)
  })

  it.each<ReviewStatus>(['DISABLED', 'PENDING', 'PASS', 'FAILED'])(
    'has no source location entry when %s has no findings',
    async (status) => {
      const html = await renderReview(
        status,
        status === 'DISABLED'
          ? null
          : {
              review_id: 'review-a',
              trigger: 'manual',
              requested_at_ms: 2000,
              completed_at_ms: status === 'PENDING' ? null : 3000,
              model: 'model',
              result:
                status === 'PASS'
                  ? { decision: 'PASS', summary: '无疑点', findings: [] }
                  : null,
              error:
                status === 'FAILED'
                  ? { code: 'ReviewApiFailed', message: 'provider error' }
                  : null,
            },
      )
      expect(html).not.toContain('查看第')
      if (status === 'FAILED') {
        expect(html).toContain('失败代码：ReviewApiFailed')
        expect(html).not.toContain('存疑')
      }
    },
  )
})
