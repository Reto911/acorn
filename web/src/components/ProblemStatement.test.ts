import { createSSRApp } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { describe, expect, it } from 'vitest'
import ProblemStatement from './ProblemStatement.vue'

function render(statement: string) {
  return renderToString(createSSRApp(ProblemStatement, { statement }))
}

describe('problem statement', () => {
  it('renders headings, lists, links and fenced samples', async () => {
    const html = await render(
      '# 两数求和\n\n输入两个整数。\n\n- 输出和\n\n[说明](https://example.com)\n\n```text\n1 2\n```',
    )
    expect(html).toContain('<h1>两数求和</h1>')
    expect(html).toContain('<li>输出和</li>')
    expect(html).toContain('<a href="https://example.com">说明</a>')
    expect(html).toContain(
      '<pre><code class="language-text">1 2\n</code></pre>',
    )
  })

  it('keeps raw HTML and Vue expressions inert and rejects dangerous links', async () => {
    const html = await render(
      '<script>alert(1)</script>\n\n<img src=x onerror="alert(1)">\n\n{{ 1 + 1 }}\n\n[危险](javascript:alert(1))',
    )
    expect(html).toContain('&lt;script&gt;alert(1)&lt;/script&gt;')
    expect(html).toContain('&lt;img')
    expect(html).toContain('{{ 1 + 1 }}')
    expect(html).not.toMatch(/<script|<img|href="javascript:/)
  })
})
