import { describe, expect, test } from 'vitest'
import { formatDate, parseMajorToMinor } from './money'

describe('parseMajorToMinor', () => {
  test('dot and comma decimals', () => {
    expect(parseMajorToMinor('25.50')).toBe(2550)
    expect(parseMajorToMinor('25,50')).toBe(2550)
    expect(parseMajorToMinor('25')).toBe(2500)
    expect(parseMajorToMinor(' 25,5 ')).toBe(2550)
  })

  test('thousand separators', () => {
    expect(parseMajorToMinor('1.234,56')).toBe(123456)
    expect(parseMajorToMinor('1,234.56')).toBe(123456)
    expect(parseMajorToMinor('1.234.567')).toBe(123456700)
  })

  test('currency symbols and negatives', () => {
    expect(parseMajorToMinor('€ 25.50')).toBe(2550)
    expect(parseMajorToMinor('-25.50')).toBe(-2550)
  })

  test('rejects garbage and over-precise fractions', () => {
    expect(parseMajorToMinor('')).toBeNull()
    expect(parseMajorToMinor('abc')).toBeNull()
    expect(parseMajorToMinor('25.505', 'EUR')).toBeNull()
  })

  test('zero-decimal currency', () => {
    expect(parseMajorToMinor('1234', 'JPY')).toBe(1234)
    expect(parseMajorToMinor('12.34', 'JPY')).toBeNull()
  })
})

describe('formatDate', () => {
  test('renders ISO strings as dd/mm/yyyy', () => {
    expect(formatDate('2026-03-15')).toBe('15/03/2026')
    expect(formatDate('2026-03-15T00:00:00')).toBe('15/03/2026')
  })

  test('renders {year, month, day} objects as dd/mm/yyyy', () => {
    expect(formatDate({ year: 2026, month: 'March', day: 5 })).toBe('05/03/2026')
    expect(formatDate({ year: 2026, month: 3, day: 5 })).toBe('05/03/2026')
  })

  test('leaves non-date strings untouched', () => {
    expect(formatDate('yesterday')).toBe('yesterday')
  })
})
