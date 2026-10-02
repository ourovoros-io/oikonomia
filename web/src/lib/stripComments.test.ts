import { describe, expect, test } from 'vitest'
import { stripComments } from './stripComments.testutil'

describe('stripComments', () => {
  test('blanks a line comment and keeps the code before it', () => {
    expect(stripComments('run() // note')).toBe('run()        ')
  })

  test('blanks a block comment over several lines and keeps the newlines', () => {
    const stripped = stripComments('a /* one\n two */ b')

    expect(stripped.split('\n')).toHaveLength(2)
    expect(stripped).not.toContain('one')
    expect(stripped).not.toContain('two')
    expect(stripped.startsWith('a ')).toBe(true)
    expect(stripped.endsWith(' b')).toBe(true)
  })

  test.each([
    ['a double-quoted string', 'const a = "x // y"; show(err.message)'],
    ['a single-quoted string', "const a = 'x // y'; show(err.message)"],
    ['a template string', 'const a = `x // y`; show(err.message)'],
    ['a string holding a block comment opener', 'const a = "x /* y"; show(err.message)'],
    ['a string with an escaped quote', "const a = 'it\\'s // here'; show(err.message)"],
    ['a template with an expression', 'const a = `${b} // ${c}`; show(err.message)'],
    ['a template inside a template expression', 'const a = `${`x // y`}`; show(err.message)'],
    ['an object literal inside a template expression', 'const a = `${{ k: 1 }.k} // z`; show(err.message)'],
  ])('keeps code after %s', (_name, source) => {
    expect(stripComments(source)).toBe(source)
  })

  test('still blanks a comment that follows a string', () => {
    expect(stripComments("const a = 'x' // err.message")).not.toContain('err.message')
  })

  test('blanks a comment inside a template expression', () => {
    expect(stripComments('`${a /* err.message */}`')).not.toContain('err.message')
  })

  test('a stray apostrophe ends at the end of the line', () => {
    // JSX text such as: <p>don't</p>
    const stripped = stripComments("<p>don't</p>\n// err.message")

    expect(stripped).not.toContain('err.message')
  })

  test('keeps the line count', () => {
    const source = '// a\n/* b\n c */\nd'

    expect(stripComments(source).split('\n')).toHaveLength(source.split('\n').length)
  })
})
