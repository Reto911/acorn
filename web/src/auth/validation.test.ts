import { describe, expect, it } from 'vitest'
import {
  normalizeDisplayName,
  normalizeUsername,
  validateDisplayName,
  validatePassword,
  validateUsername,
} from './validation'

describe('authentication input rules match Rust', () => {
  it('normalizes only the five ASCII whitespace characters and ASCII case', () => {
    expect(normalizeUsername(' \t\n\r\fTeST_1 \t\n\r\f')).toBe('test_1')
    expect(validateUsername(' \tTeST_1\r\n')).toBeUndefined()
    expect(validateUsername('a'.repeat(32))).toBeUndefined()
    for (const value of [
      'ab',
      'a'.repeat(33),
      'ab c',
      '用户abc',
      '\vabc',
      'abc\v',
      '\u0085abc',
      'abc\u2028',
    ])
      expect(validateUsername(value)).toBeDefined()
  })

  it('distinguishes an omitted display name from explicit blank and trims Unicode White_Space', () => {
    expect(normalizeDisplayName(undefined)).toBeUndefined()
    expect(validateDisplayName(undefined)).toBeUndefined()
    expect(normalizeDisplayName('\u0085\u3000 张三 \t')).toBe('张三')
    expect(validateDisplayName('\v张三\v')).toBeUndefined()
    for (const value of [
      '',
      ' \u0085\u3000',
      '张\n三',
      '张\u007f三',
      '张\u0085三',
    ])
      expect(validateDisplayName(value)).toBeDefined()
    // BOM is not Rust White_Space and is preserved instead of silently trimmed.
    expect(normalizeDisplayName('\ufeff张三\ufeff')).toBe('\ufeff张三\ufeff')
  })

  it('counts Unicode scalar values for display names, including combining characters', () => {
    expect(validateDisplayName('😀'.repeat(64))).toBeUndefined()
    expect(validateDisplayName('😀'.repeat(65))).toBeDefined()
    expect(validateDisplayName('e\u0301'.repeat(32))).toBeUndefined()
    expect(validateDisplayName('e\u0301'.repeat(33))).toBeDefined()
    expect(validateDisplayName('name\ud800')).toBeDefined()
  })

  it('accepts Unicode passwords at scalar and UTF-8 limits without composition rules', () => {
    expect(validatePassword('a'.repeat(8))).toBeUndefined()
    expect(validatePassword('我'.repeat(8))).toBeUndefined()
    expect(validatePassword('😀'.repeat(128))).toBeUndefined()
    expect(new TextEncoder().encode('😀'.repeat(128)).length).toBe(512)
    expect(validatePassword('😀'.repeat(129))).toBeDefined()
    expect(validatePassword('😀'.repeat(7))).toBeDefined()
    expect(validatePassword('e\u0301'.repeat(4))).toBeUndefined()
    expect(validatePassword('password\ud800')).toBeDefined()
  })

  it('rejects all Rust Unicode White_Space without trimming the password', () => {
    const whitespace =
      '\t\n\v\f\r \u0085\u00a0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200a\u2028\u2029\u202f\u205f\u3000'
    for (const character of whitespace) {
      expect(validatePassword(`password${character}`)).toBeDefined()
      expect(validatePassword(`${character}password`)).toBeDefined()
    }
    expect(validatePassword('password\ufeff')).toBeUndefined()
    expect(validatePassword('password\u200b')).toBeUndefined()
  })
})
