/**
 * Shared by the source-scanning guard tests (`i18n.catalog.test.ts` and
 * `noRawErrorMessage.test.ts`). Both skip files named `*.testutil.ts`.
 */

type Mode = 'code' | 'single' | 'double' | 'template'

/** Replace every character but newlines with a space. */
function blank(text: string): string {
  return text.replace(/[^\n]/g, ' ')
}

/**
 * Blank out comments, keeping every newline so reported line numbers stay
 * right. It reads the source the way the language does: a `//` or a slash and
 * star inside a single, double or template quoted string is text, not a
 * comment, and an expression inside `${...}` is code again.
 *
 * Not a parser. A regular expression literal that contains a quote character
 * can fool it for the rest of that line (strings other than templates end at a
 * newline), which is why the scans that use it are tripwires.
 */
export function stripComments(source: string): string {
  let result = ''
  let mode: Mode = 'code'
  let index = 0

  // One entry per open `${`: how many `{` are open inside it.
  const expressionDepths: number[] = []

  while (index < source.length) {
    const char = source[index]
    const next = source[index + 1]

    if (mode === 'code') {
      if (char === '/' && next === '/') {
        const newline = source.indexOf('\n', index)
        const end = newline === -1 ? source.length : newline

        result += blank(source.slice(index, end))
        index = end
        continue
      }

      if (char === '/' && next === '*') {
        const close = source.indexOf('*/', index + 2)
        const end = close === -1 ? source.length : close + 2

        result += blank(source.slice(index, end))
        index = end
        continue
      }

      if (char === "'") mode = 'single'
      else if (char === '"') mode = 'double'
      else if (char === '`') mode = 'template'
      else if (char === '{' && expressionDepths.length > 0) {
        expressionDepths[expressionDepths.length - 1] += 1
      } else if (char === '}' && expressionDepths.length > 0) {
        if (expressionDepths[expressionDepths.length - 1] === 0) {
          expressionDepths.pop()
          mode = 'template'
        } else {
          expressionDepths[expressionDepths.length - 1] -= 1
        }
      }

      result += char
      index += 1
      continue
    }

    if (char === '\\') {
      result += source.slice(index, index + 2)
      index += 2
      continue
    }

    if (mode === 'template' && char === '$' && next === '{') {
      expressionDepths.push(0)
      mode = 'code'
      result += '${'
      index += 2
      continue
    }

    const closes =
      (mode === 'single' && char === "'") ||
      (mode === 'double' && char === '"') ||
      (mode === 'template' && char === '`')
    // Only a template literal may span lines; a stray quote ends at the line.
    const unterminated = mode !== 'template' && char === '\n'

    if (closes || unterminated) mode = 'code'

    result += char
    index += 1
  }

  return result
}
