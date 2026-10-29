import { describe, expect, it } from 'vitest'
import { sourcePresentation } from './sourcePresentation'

describe('source presentation', () => {
  it.each([
    { name: 'empty source', source: '', text: '', hasStandaloneCR: false },
    {
      name: 'LF lines with Chinese, tabs, empty lines and a trailing newline',
      source: '中文\tvalue\n\n尾行\n',
      text: '中文\tvalue\n\n尾行\n',
      hasStandaloneCR: false,
    },
    {
      name: 'CRLF lines with empty lines and a trailing newline',
      source: '中文\tvalue\r\n\r\n尾行\r\n',
      text: '中文\tvalue\r\n\r\n尾行\r\n',
      hasStandaloneCR: false,
    },
    {
      name: 'standalone and consecutive carriage returns',
      source: 'a\rb\r\rc\r',
      text: 'a␍b␍␍c␍',
      hasStandaloneCR: true,
    },
    {
      name: 'mixed LF, CRLF and standalone CR including CR before CRLF',
      source: '中文\t\r值\r\n\n尾\r\r\n',
      text: '中文\t␍值\r\n\n尾␍\r\n',
      hasStandaloneCR: true,
    },
  ])('handles $name', ({ source, text, hasStandaloneCR }) => {
    expect(sourcePresentation(source)).toEqual({ text, hasStandaloneCR })
  })
})
