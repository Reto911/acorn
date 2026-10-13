import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { describe, expect, it } from 'vitest'
import type { Submission } from '../api/types'
import SubmissionOutcome from './SubmissionOutcome.vue'

const identity = {
  submission_id: 'result',
  problem_id: 1,
  version_no: 1,
  language_id: 'c' as const,
  created_at_ms: 1000,
  terminal_at_ms: 2000,
  review_status: 'DISABLED' as const,
  review: null,
}

function render(submission: Submission) {
  return renderToString(createSSRApp(SubmissionOutcome, { submission }))
}

describe('persisted submission outcomes', () => {
  it('renders AC without inventing per-case details or resource totals', async () => {
    const html = await render({
      ...identity,
      state: 'finished',
      verdict: 'AC',
      terminal_details: null,
    })
    expect(html).toContain('通过（AC）')
    expect(html).not.toMatch(/首个失败用例|CPU 时间|编译诊断|stdout/)
  })

  it('decodes CE bytes as UTF-8 text, escapes HTML and shows truncation without case details', async () => {
    const diagnostic = '编译错误 <script>private-output</script>\n'
    const html = await render({
      ...identity,
      state: 'finished',
      verdict: 'CE',
      terminal_details: {
        compile_diagnostic_base64: Buffer.from(diagnostic).toString('base64'),
        compile_diagnostic_truncated: true,
      },
    })
    expect(html).toContain('编译错误（CE）')
    expect(html).toContain(
      '编译错误 &lt;script&gt;private-output&lt;/script&gt;',
    )
    expect(html).toContain('输出已截断')
    expect(html).not.toMatch(/<script>|首个失败用例|CPU 时间/)
  })

  it('renders only the failed case, preserving zero and missing measurements with correct units', async () => {
    const html = await render({
      ...identity,
      state: 'finished',
      verdict: 'WA',
      terminal_details: {
        failed_case: {
          case_index: 2,
          stdout_base64: '5Lit5paH/w==',
          stderr_base64: '',
          stdout_truncated: false,
          stderr_truncated: true,
          resources: {
            cpu_time_us: 0,
            wall_time_us: 2500,
            peak_memory_bytes: 1048576,
          },
          exit_code: 0,
        },
      },
    })
    expect(html).toContain('首个失败用例 #2')
    expect(html).toContain('中文�')
    expect(html).toContain('0 ms')
    expect(html).toContain('2.5 ms')
    expect(html).toContain('1 MiB')
    expect(html).toContain('<dt>退出码</dt><dd>0</dd>')
    expect(html).toContain('<dt>信号</dt><dd>未提供</dd>')
    expect(html).toContain('输出为空')
    expect(html).toContain('输出已截断')
    const absent = await render({
      ...identity,
      state: 'finished',
      verdict: 'RE',
      terminal_details: {
        failed_case: {
          case_index: 1,
          stdout_base64: '',
          stderr_base64: '',
          stdout_truncated: false,
          stderr_truncated: false,
        },
      },
    })
    expect(absent).toContain('<dt>CPU 时间</dt><dd>未提供</dd>')
    expect(absent).not.toContain('0 ms')
  })

  it('shows persisted execution failure separately from verdicts and explains restart interruption', async () => {
    const html = await render({
      ...identity,
      state: 'failed',
      verdict: null,
      terminal_details: {
        code: 'SchedulerInterrupted',
        diagnostic: '<error> Backend restarted',
      },
    })
    expect(html).toContain('执行失败')
    expect(html).toContain('SchedulerInterrupted')
    expect(html).toContain('后端重启导致此次执行中断')
    expect(html).toContain('&lt;error&gt; Backend restarted')
    expect(html).not.toMatch(/（AC）|（RE）|首个失败用例|<error>/)
  })
})
