import { describe, expect, test } from 'vitest'
import { FILE_TEXT_LIMIT, shortenFileText, withShortFileText } from './fileText'

describe('shortenFileText', () => {
  test('leaves a short cell as it is', () => {
    expect(shortenFileText('31/31/2026')).toBe('31/31/2026')
    expect(shortenFileText('a'.repeat(FILE_TEXT_LIMIT))).toBe('a'.repeat(FILE_TEXT_LIMIT))
  })

  test('cuts a long cell at the limit and marks the cut', () => {
    const shown = shortenFileText('a'.repeat(FILE_TEXT_LIMIT + 1))

    expect(shown).toBe(`${'a'.repeat(FILE_TEXT_LIMIT)}…`)
  })

  test('puts a cell with line breaks on one line', () => {
    expect(shortenFileText('  first\r\nsecond\t\tthird ')).toBe('first second third')
  })

  test('does not leave a space before the ellipsis', () => {
    const shown = shortenFileText(`${'a'.repeat(FILE_TEXT_LIMIT - 1)} bcd`)

    expect(shown).toBe(`${'a'.repeat(FILE_TEXT_LIMIT - 1)}…`)
  })

  test('counts code points, so a cut never splits a character', () => {
    const shown = shortenFileText('\u{1D11E}'.repeat(FILE_TEXT_LIMIT + 5))

    expect(Array.from(shown)).toHaveLength(FILE_TEXT_LIMIT + 1)
    expect(shown.endsWith('\u{1D11E}…')).toBe(true)
  })

  test('keeps markup as the text it is', () => {
    expect(shortenFileText('<img src=x onerror=alert(1)>')).toBe('<img src=x onerror=alert(1)>')
  })
})

describe('withShortFileText', () => {
  test('shortens the values that come from a file and no others', () => {
    const long = 'x'.repeat(FILE_TEXT_LIMIT * 2)

    expect(withShortFileText({ value: long, column: long, problem: long, name: long })).toEqual({
      value: `${'x'.repeat(FILE_TEXT_LIMIT)}…`,
      column: `${'x'.repeat(FILE_TEXT_LIMIT)}…`,
      problem: long,
      name: long,
    })
  })
})
