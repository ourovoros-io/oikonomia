import { describe, expect, test } from 'vitest'

import { arcEndDeg, arcFraction, arcPath, arcPoint, formatPercentFromBps } from './arc'

describe('arcFraction', () => {
  test('maps basis points onto the arc, clamped', () => {
    expect(arcFraction(6900)).toBe(0.69)
    expect(arcFraction(0)).toBe(0)
    expect(arcFraction(-500)).toBe(0)
    expect(arcFraction(25000)).toBe(1)
    expect(arcFraction(2500, 5000)).toBe(0.5)
  })

  test('has no fraction for a missing value', () => {
    expect(arcFraction(null)).toBeNull()
    expect(arcFraction(undefined)).toBeNull()
    expect(arcFraction(Number.NaN)).toBeNull()
  })
})

describe('arc drawing', () => {
  test('sweeps 270 degrees clockwise from the lower left', () => {
    expect([arcEndDeg(0), arcEndDeg(0.5), arcEndDeg(1)]).toEqual([135, 270, 405])
    const start = arcPoint(35, 35, 26, 135)
    expect(start.x).toBeCloseTo(16.615, 2)
    expect(start.y).toBeCloseTo(53.385, 2)
  })

  test('uses the large-arc flag only past 180 degrees', () => {
    expect(arcPath(35, 35, 26, 135, 405)).toBe('M16.62 53.38A26 26 0 1 1 53.38 53.38')
    expect(arcPath(35, 35, 26, 135, 270)).toBe('M16.62 53.38A26 26 0 0 1 35.00 9.00')
  })
})

describe('formatPercentFromBps', () => {
  test('shows one decimal in the reader locale', () => {
    expect(formatPercentFromBps(6900, 'en')).toBe('69.0')
    expect(formatPercentFromBps(6900, 'el')).toBe('69,0')
  })

  test('signs a change only when asked', () => {
    expect(formatPercentFromBps(2000, 'en', { signed: true })).toBe('+20.0')
    expect(formatPercentFromBps(-2000, 'en', { signed: true })).toBe('-20.0')
    expect(formatPercentFromBps(0, 'en', { signed: true })).toBe('0.0')
    expect(formatPercentFromBps(2000, 'en')).toBe('20.0')
  })
})
