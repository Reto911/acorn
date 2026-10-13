import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { describe, expect, it, vi } from 'vitest'
import type { Submission } from '../api/types'
import SubmissionReview from './SubmissionReview.vue'

// Node SSR verifies actual escaped markup without loading browser-only style entry points.
vi.mock('element-plus/es/components/button/style/css', () => ({}))
vi.mock('element-plus/es/components/alert/style/css', () => ({}))
vi.mock('element-plus/es/components/empty/style/css', () => ({}))

describe('persisted review presentation', () => {
  it('escapes all model text, shows inclusive source locations and keeps doubtful distinct from failed', async () => {
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
      review_status: 'DOUBTFUL',
      review: {
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
          ],
        },
      },
    }
    const html = await renderToString(
      createSSRApp(SubmissionReview, {
        submission,
        sending: false,
        canStart: true,
        refreshRequired: false,
        error: null,
      }),
    )
    expect(html).toContain('存疑')
    expect(html).toContain('第 2–4 行（含首尾）')
    expect(html).toContain('针对测试数据')
    expect(html).toContain('&lt;script&gt;alert(1)&lt;/script&gt; **原文**')
    expect(html).toContain('&lt;img src=x onerror=alert(1)&gt;')
    expect(html).toContain('&lt;model&gt;')
    expect(html).toContain('[说明](javascript:alert(1))')
    expect(html).not.toMatch(/<script>|<img|<a |<strong>|失败代码/)
  })
})
