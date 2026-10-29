/** Keep LF review line numbers while making standalone carriage returns visible. */
export function sourcePresentation(source: string): {
  text: string
  hasStandaloneCR: boolean
} {
  return {
    text: source.replace(/\r(?!\n)/g, '␍'),
    hasStandaloneCR: /\r(?!\n)/.test(source),
  }
}
