/** Rust char::is_whitespace uses Unicode White_Space, unlike JavaScript trim. */
const whitespace =
  '[\\u0009-\\u000d\\u0020\\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000]'
const unicodeWhitespace = new RegExp(whitespace, 'u')
const unicodeTrim = new RegExp(`^${whitespace}+|${whitespace}+$`, 'gu')
const asciiTrim = /^[\t\n\f\r ]+|[\t\n\f\r ]+$/g
const control = /\p{Cc}/u
const unpairedSurrogate = /[\ud800-\udfff]/u

/** Apply the same ASCII trimming and ASCII case conversion as the backend. */
export function normalizeUsername(value: string) {
  return value
    .replace(asciiTrim, '')
    .replace(/[A-Z]/g, (letter) => letter.toLowerCase())
}

/** Preserve an omitted name; an explicit empty name remains invalid. */
export function normalizeDisplayName(value: string | undefined) {
  return value?.replace(unicodeTrim, '')
}

/** Return a field error, without reflecting the supplied username. */
export function validateUsername(value: string): string | undefined {
  const normalized = normalizeUsername(value)
  if (
    normalized.length < 3 ||
    normalized.length > 32 ||
    /[^a-z0-9_]/u.test(normalized)
  )
    return '用户名须为 3–32 个英文字母、数字或下划线。'
}

/** Validate Unicode scalar count and controls after Rust-compatible trimming. */
export function validateDisplayName(
  value: string | undefined,
): string | undefined {
  const normalized = normalizeDisplayName(value)
  if (normalized === undefined) return
  const count = [...normalized].length
  if (
    count < 1 ||
    count > 64 ||
    control.test(normalized) ||
    unpairedSurrogate.test(normalized)
  )
    return '显示名须为 1–64 个 Unicode 字符，且不能含控制字符。'
}

/** Validate the original password without trimming, rewriting or reflecting it. */
export function validatePassword(value: string): string | undefined {
  const count = [...value].length
  if (
    count < 8 ||
    count > 128 ||
    new TextEncoder().encode(value).length > 512 ||
    unicodeWhitespace.test(value) ||
    unpairedSurrogate.test(value)
  )
    return '密码须为 8–128 个 Unicode 字符、最多 512 字节，且不能含空白。'
}
