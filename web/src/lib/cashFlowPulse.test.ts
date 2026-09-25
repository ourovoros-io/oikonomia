import { describe, expect, test } from 'vitest'

import type { CashFlowBucket, CashFlowSeries } from './api'
import { barPath, bucketAt, bucketDate, pulseLayout, zeroSplit } from './cashFlowPulse'
import { LEDGER_IN_STOPS, LEDGER_OUT_STOPS } from './ledgerColours'

function bucket(day: number, income: number, expenses: number): CashFlowBucket {
  const iso = `2026-08-${String(day).padStart(2, '0')}`
  return {
    start: iso,
    end: iso,
    income_minor: income,
    expenses_minor: expenses,
    cumulative_income_minor: 0,
    cumulative_expenses_minor: 0,
  }
}

function seriesOf(buckets: CashFlowBucket[]): CashFlowSeries {
  return {
    entity_id: 'e1',
    from: buckets[0]?.start ?? '2026-08-01',
    to: buckets[buckets.length - 1]?.end ?? '2026-08-01',
    granularity: 'day',
    total_income_minor: 0,
    total_expenses_minor: 0,
    net_minor: 0,
    buckets,
  }
}

// In 300 on day 2, out 100 on day 3, a quiet day either side.
const august = seriesOf([bucket(1, 0, 0), bucket(2, 300, 0), bucket(3, 0, 100), bucket(4, 0, 0)])

describe('the Ledger stops', () => {
  test('are the spec gradient stops, in and out', () => {
    expect(LEDGER_IN_STOPS).toEqual(['#27bf93', '#1ba39a', '#1e8db0'])
    expect(LEDGER_OUT_STOPS).toEqual(['#f07a45', '#e8603f', '#d64a5a'])
  })
})

describe('zeroSplit', () => {
  test('gives in the share of the height its peak needs, within 30-80%', () => {
    expect(zeroSplit(0, 0)).toBe(0.74)
    expect(zeroSplit(300, 100)).toBe(0.75)
    expect(zeroSplit(100, 0)).toBe(0.8)
    expect(zeroSplit(0, 100)).toBe(0.3)
    expect(zeroSplit(1, 99)).toBe(0.3)
  })
})

describe('pulseLayout', () => {
  // Plot = 208 - 2 * 4 = 200; split 0.75, so the zero line is at 4 + 150.
  const layout = pulseLayout(august, { width: 408, height: 208 })

  test('puts one column per bucket, centred in equal bands', () => {
    expect(layout.band).toBe(100)
    expect(layout.bars.map((bar) => bar.x)).toEqual([54, 154, 254, 354])
    expect(layout.barWidth).toBe(12)
  })

  test('in rises and out hangs on one scale, filling its room exactly', () => {
    expect(layout.zeroY).toBe(154)
    expect(layout.bars[1]?.inHeight).toBe(150)
    expect(layout.bars[2]?.outHeight).toBe(50)
    expect(layout.bars[1]?.outHeight).toBe(0)
  })

  test('a quiet bucket draws nothing, a tiny one still shows', () => {
    const tiny = pulseLayout(seriesOf([bucket(1, 100_000, 0), bucket(2, 1, 0), bucket(3, 0, 0)]), {
      width: 300,
      height: 100,
    })
    expect(tiny.bars[1]?.inHeight).toBe(2)
    expect(tiny.bars[2]?.inHeight).toBe(0)
    expect(tiny.bars[2]?.outHeight).toBe(0)
  })

  test('names the peak bucket on each side', () => {
    expect(layout.peakIn).toBe(1)
    expect(layout.peakOut).toBe(2)
    const quiet = pulseLayout(seriesOf([bucket(1, 0, 0)]), { width: 100, height: 50 })
    expect(quiet.peakIn).toBeNull()
    expect(quiet.peakOut).toBeNull()
  })

  test('a negative amount is treated as nothing, never drawn across the line', () => {
    const odd = pulseLayout(seriesOf([bucket(1, -50, -20), bucket(2, 10, 10)]), {
      width: 200,
      height: 100,
    })
    expect(odd.bars[0]?.inHeight).toBe(0)
    expect(odd.bars[0]?.outHeight).toBe(0)
  })

  test('no series lays out no bars and rests the zero line', () => {
    const empty = pulseLayout(null, { width: 400, height: 108 })
    expect(empty.bars).toEqual([])
    expect(empty.zeroY).toBeCloseTo(4 + 100 * 0.74)
  })

  test('a narrow chart never draws a bar wider than its column', () => {
    const days = Array.from({ length: 90 }, (_, index) => bucket((index % 28) + 1, 10, 10))
    const narrow = pulseLayout(seriesOf(days), { width: 188, height: 72 })
    expect(narrow.barWidth).toBeLessThanOrEqual(narrow.band)
  })
})

describe('bucketAt', () => {
  const layout = pulseLayout(august, { width: 408, height: 208 })

  test('maps a pointer x to the column under it, clamped to the ends', () => {
    expect(bucketAt(layout, 160)).toBe(1)
    expect(bucketAt(layout, 205)).toBe(2)
    expect(bucketAt(layout, -30)).toBe(0)
    expect(bucketAt(layout, 999)).toBe(3)
  })

  test('an empty chart has nothing under the pointer', () => {
    expect(bucketAt(pulseLayout(null, { width: 400, height: 100 }), 50)).toBeNull()
  })
})

describe('barPath', () => {
  test('sits flat on the zero line and rounds only the far end', () => {
    expect(barPath(50, 10, 100, 40, -1)).toBe('M45,100V64Q45,60 49,60H51Q55,60 55,64V100Z')
    expect(barPath(50, 10, 100, 40, 1)).toBe('M45,100V136Q45,140 49,140H51Q55,140 55,136V100Z')
  })

  test('a bar shorter than the corner radius stays that short', () => {
    expect(barPath(50, 10, 100, 2, -1)).toBe('M45,100V100Q45,98 47,98H53Q55,98 55,100V100Z')
  })
})

describe('bucketDate', () => {
  test('reads an ISO day as that calendar day, in local time', () => {
    const date = bucketDate('2026-09-05')
    expect([date.getFullYear(), date.getMonth(), date.getDate(), date.getDay()]).toEqual([
      2026, 8, 5, 6,
    ])
  })
})
