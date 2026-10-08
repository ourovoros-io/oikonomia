import { describe, expect, test } from 'vitest'
import { bookDotColour } from './bookColour'

describe('bookDotColour', () => {
  test('depends on the id alone and stays within the palette', () => {
    expect(bookDotColour('0b7c1c6e')).toBe(bookDotColour('0b7c1c6e'))
    expect(bookDotColour('0b7c1c6e')).toMatch(/^var\(--viz-[1-8]\)$/)
  })

  test('uses more than one slot across different ids', () => {
    const slots = new Set(['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'].map(bookDotColour))
    expect(slots.size).toBeGreaterThan(1)
  })
})
