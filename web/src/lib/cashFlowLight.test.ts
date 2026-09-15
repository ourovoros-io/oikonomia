import { describe, expect, test } from 'vitest'

import type { CashFlowBucket, CashFlowSeries } from './api'
import {
  LEDGER_IN_STOPS,
  LEDGER_OUT_STOPS,
  cumulativeAt,
  ease,
  ledgerColourAt,
  lightGeometry,
  zeroSplit,
} from './cashFlowLight'

function bucket(day: number, income: number, expenses: number, cumIn: number, cumOut: number): CashFlowBucket {
  const iso = `2026-08-${String(day).padStart(2, '0')}`
  return {
    start: iso,
    end: iso,
    income_minor: income,
    expenses_minor: expenses,
    cumulative_income_minor: cumIn,
    cumulative_expenses_minor: cumOut,
  }
}

function seriesOf(buckets: CashFlowBucket[]): CashFlowSeries {
  const last = buckets[buckets.length - 1]
  return {
    entity_id: 'e1',
    from: buckets[0]?.start ?? '2026-08-01',
    to: last?.end ?? '2026-08-01',
    granularity: 'day',
    total_income_minor: last?.cumulative_income_minor ?? 0,
    total_expenses_minor: last?.cumulative_expenses_minor ?? 0,
    net_minor: (last?.cumulative_income_minor ?? 0) - (last?.cumulative_expenses_minor ?? 0),
    buckets,
  }
}

// In 300 on day 2, out 100 on day 3.
const august = seriesOf([
  bucket(1, 0, 0, 0, 0),
  bucket(2, 300, 0, 300, 0),
  bucket(3, 0, 100, 300, 100),
  bucket(4, 0, 0, 300, 100),
])

const layout = { width: 400, height: 200, samples: 41 }
// top = 200 * 0.16 = 32; plot = 200 - 32 - 4 = 164.
const TOP = 32
const BOTTOM = 196

describe('the Ledger stops', () => {
  test('are the spec gradient stops, in and out', () => {
    expect(LEDGER_IN_STOPS).toEqual(['#27bf93', '#1ba39a', '#1e8db0'])
    expect(LEDGER_OUT_STOPS).toEqual(['#f07a45', '#e8603f', '#d64a5a'])
  })

  test('ledgerColourAt walks the three stops', () => {
    expect(ledgerColourAt(LEDGER_IN_STOPS, 0, 0.5)).toBe('rgba(39,191,147,0.5)')
    expect(ledgerColourAt(LEDGER_IN_STOPS, 0.5)).toBe('rgba(27,163,154,1)')
    expect(ledgerColourAt(LEDGER_IN_STOPS, 1)).toBe('rgba(30,141,176,1)')
    expect(ledgerColourAt(LEDGER_OUT_STOPS, 2)).toBe('rgba(214,74,90,1)')
  })
})

describe('easing and running totals', () => {
  test('ease is flat outside the step and an S-curve across it', () => {
    expect([ease(-1), ease(0), ease(0.5), ease(1), ease(2)]).toEqual([0, 0, 0.5, 1, 1])
  })

  test('the edge passes through every bucket running total at the bucket end', () => {
    const amounts = [0, 300, 0, 0]
    expect([0, 1, 2, 3, 4].map((u) => cumulativeAt(amounts, u))).toEqual([0, 0, 300, 300, 300])
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

describe('lightGeometry', () => {
  test('in rises above the zero line and out hangs below it, edge to edge', () => {
    const g = lightGeometry(august, layout)

    expect(g.inEdge).toHaveLength(41)
    expect(g.inEdge[0]?.x).toBe(0)
    expect(g.inEdge[40]?.x).toBe(400)
    expect(g.zeroY).toBeCloseTo(TOP + 164 * 0.75)
    expect(g.inEdge[0]?.y).toBeCloseTo(g.zeroY)
    expect(g.inEdge.every((p) => p.y <= g.zeroY + 1e-9)).toBe(true)
    expect(g.outEdge.every((p) => p.y >= g.zeroY - 1e-9)).toBe(true)
  })

  test('both sides share one scale and fill the canvas without leaving it', () => {
    const g = lightGeometry(august, layout)
    const inLift = g.zeroY - (g.inEdge[40]?.y ?? 0)
    const outLift = (g.outEdge[40]?.y ?? 0) - g.zeroY

    expect(inLift / outLift).toBeCloseTo(3)
    expect(g.inEdge[40]?.y).toBeCloseTo(TOP)
    expect(g.outEdge[40]?.y).toBeCloseTo(BOTTOM)
  })

  test('a month where out beats in still fits, on the same scale', () => {
    const heavy = seriesOf([bucket(1, 100, 0, 100, 0), bucket(2, 0, 400, 100, 400)])
    const g = lightGeometry(heavy, layout)
    const inLift = g.zeroY - (g.inEdge[40]?.y ?? 0)
    const outLift = (g.outEdge[40]?.y ?? 0) - g.zeroY

    expect(outLift / inLift).toBeCloseTo(4)
    for (const p of [...g.inEdge, ...g.outEdge]) {
      expect(p.y).toBeGreaterThanOrEqual(TOP - 1e-9)
      expect(p.y).toBeLessThanOrEqual(BOTTOM + 1e-9)
    }
  })

  test('no series draws a flat line at rest', () => {
    const g = lightGeometry(null, layout)

    expect(g.zeroY).toBeCloseTo(TOP + 164 * 0.74)
    expect([...g.inEdge, ...g.outEdge].every((p) => Math.abs(p.y - g.zeroY) < 1e-9)).toBe(true)
  })

  test('a running total below zero stays on the zero line', () => {
    const refunds = seriesOf([bucket(1, 0, 0, 0, 0), bucket(2, -50, 0, -50, 0)])
    const g = lightGeometry(refunds, layout)

    expect(g.inEdge.every((p) => Math.abs(p.y - g.zeroY) < 1e-9)).toBe(true)
  })

  test('holding still ignores time; breathing moves the edge but never across the line', () => {
    const still = lightGeometry(august, { ...layout, time: 0, breathe: 0 })
    const later = lightGeometry(august, { ...layout, time: 5, breathe: 0 })
    expect(later).toEqual(still)

    const a = lightGeometry(august, { ...layout, time: 0, breathe: 1 })
    const b = lightGeometry(august, { ...layout, time: 1.3, breathe: 1 })
    expect(b.inEdge.map((p) => p.y)).not.toEqual(a.inEdge.map((p) => p.y))
    expect(b.inEdge.every((p) => p.y <= b.zeroY + 1e-9)).toBe(true)
    expect(b.outEdge.every((p) => p.y >= b.zeroY - 1e-9)).toBe(true)
  })

  test('samples in O(1) yet matches a cumulativeAt reference across ~40 mixed buckets', () => {
    const n = 40
    let cumIn = 0
    let cumOut = 0
    const buckets = Array.from({ length: n }, (_, i) => {
      // Mixed signs: some buckets are net refunds/corrections.
      const income = (i % 5) * 37 - 60
      const expenses = (i % 7) * 29 - 40
      cumIn += income
      cumOut += expenses
      return bucket(i + 1, income, expenses, cumIn, cumOut)
    })
    const mixed = seriesOf(buckets)
    const wideLayout = { width: 500, height: 240, samples: 97 }

    const reference = referenceLightGeometry(mixed, wideLayout)
    const g = lightGeometry(mixed, wideLayout)

    expect(g.zeroY).toBeCloseTo(reference.zeroY)
    g.inEdge.forEach((p, i) => {
      expect(p.x).toBeCloseTo(reference.inEdge[i]?.x ?? NaN)
      expect(p.y).toBeCloseTo(reference.inEdge[i]?.y ?? NaN)
    })
    g.outEdge.forEach((p, i) => {
      expect(p.x).toBeCloseTo(reference.outEdge[i]?.x ?? NaN)
      expect(p.y).toBeCloseTo(reference.outEdge[i]?.y ?? NaN)
    })
  })
})

/**
 * A reference geometry built the slow way — walking every bucket for every
 * sample via `cumulativeAt` — so the fast O(1) sampler in `lightGeometry` can
 * be checked against it without duplicating its own math.
 */
function referenceLightGeometry(
  series: CashFlowSeries,
  { width, height, samples }: { width: number; height: number; samples: number },
) {
  const top = height * 0.16
  const plot = Math.max(0, height - top - 4)
  const buckets = series.buckets
  const maxIn = buckets.reduce((peak, b) => Math.max(peak, b.cumulative_income_minor), 0)
  const maxOut = buckets.reduce((peak, b) => Math.max(peak, b.cumulative_expenses_minor), 0)
  const split = zeroSplit(maxIn, maxOut)
  const zeroY = top + plot * split
  const inScale = maxIn > 0 ? (plot * split) / maxIn : Number.POSITIVE_INFINITY
  const outScale = maxOut > 0 ? (plot * (1 - split)) / maxOut : Number.POSITIVE_INFINITY
  const scale = Math.min(inScale, outScale)
  const pixelsPerMinor = Number.isFinite(scale) ? scale : 0
  const count = Math.max(2, Math.round(samples))
  const n = buckets.length

  const edge = (amounts: number[], direction: -1 | 1) => {
    const points: { x: number; y: number }[] = []
    for (let i = 0; i < count; i += 1) {
      const t = i / (count - 1)
      const value = n === 0 ? 0 : Math.max(0, cumulativeAt(amounts, t * n))
      const lift = value * pixelsPerMinor
      points.push({ x: t * width, y: zeroY + direction * lift })
    }
    return points
  }

  return {
    zeroY,
    inEdge: edge(
      buckets.map((b) => b.income_minor),
      -1,
    ),
    outEdge: edge(
      buckets.map((b) => b.expenses_minor),
      1,
    ),
  }
}
