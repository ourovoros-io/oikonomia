import { describe, expect, test } from 'vitest'
import { beginExclusive } from './guards'

describe('beginExclusive', () => {
  test('first call claims, second is refused until released', () => {
    const busyRef = { current: false }
    expect(beginExclusive(busyRef)).toBe(true)
    expect(busyRef.current).toBe(true)
    expect(beginExclusive(busyRef)).toBe(false)
    busyRef.current = false
    expect(beginExclusive(busyRef)).toBe(true)
  })
})
