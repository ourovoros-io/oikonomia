import { describe, expect, test } from 'vitest'
import { parseMajorToMinor } from './amountParse'
import vectors from './amountVectors.json'

const DECIMALS: Record<string, number> = { EUR: 2, HUF: 2, JPY: 0, KWD: 3 }

describe('parseMajorToMinor', () => {
  // The same file is read by `crates/oikonomia-core/src/csv/amount.rs`, so a
  // typed amount and a CSV cell with the same text give the same amount.
  test.each(vectors)('reads $raw in $currency as $minor', ({ raw, currency, minor }) => {
    const decimals = DECIMALS[currency]
    if (decimals === undefined) throw new Error(`amountVectors.json: no decimals for ${currency}`)

    expect(parseMajorToMinor(raw, { code: currency, decimals })).toBe(minor)
  })

  test('1,280 in yen is one thousand two hundred and eighty', () => {
    expect(parseMajorToMinor('1,280', { code: 'JPY', decimals: 0 })).toBe(1280)
  })

  test('1,234 in euros is a thousands group, as in a CSV, not 1.234', () => {
    expect(parseMajorToMinor('1,234', { code: 'EUR', decimals: 2 })).toBe(123400)
  })

  test('a number beyond what a JS integer holds exactly is refused', () => {
    expect(parseMajorToMinor('9'.repeat(20), { code: 'EUR', decimals: 2 })).toBeNull()
  })
})
