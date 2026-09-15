import type { CashFlowSeries } from './api'

/** The Ledger light for money in, left to right (spec: Rendering). */
export const LEDGER_IN_STOPS = ['#27bf93', '#1ba39a', '#1e8db0'] as const
/** The Ledger light for money out, left to right. */
export const LEDGER_OUT_STOPS = ['#f07a45', '#e8603f', '#d64a5a'] as const

export type LedgerStops = readonly [string, string, string]

export type LightPoint = { x: number; y: number }

export type LightGeometry = {
  /** Y of the zero line, in CSS pixels from the top of the canvas. */
  zeroY: number
  /** Upper edge of the in band, left to right; never below zeroY. */
  inEdge: LightPoint[]
  /** Lower edge of the out band, left to right; never above zeroY. */
  outEdge: LightPoint[]
}

export type LightLayout = {
  width: number
  height: number
  /** Points per edge, both ends included. */
  samples?: number
  /** Seconds; only matters while breathing. */
  time?: number
  /** 0 holds the light still; 1 is the full, gentle breathing amplitude. */
  breathe?: number
}

/** The top of the canvas stays clear for the text drawn above the light. */
const TOP_CLEARANCE = 0.16
const BOTTOM_CLEARANCE = 4
/** Where the zero line rests when there is nothing to draw (the mockup's). */
const RESTING_SPLIT = 0.74
const MIN_SPLIT = 0.3
const MAX_SPLIT = 0.8

/** Smoothstep: 0 before the step, 1 after it, an S-curve across it. */
export function ease(u: number): number {
  if (u <= 0) return 0
  if (u >= 1) return 1
  return u * u * (3 - 2 * u)
}

/**
 * The running total at `u`, measured in buckets from the window's start. Each
 * bucket's amount rises across its own width, so the result equals the
 * cumulative total exactly at every bucket's end.
 */
export function cumulativeAt(amounts: readonly number[], u: number): number {
  let total = 0
  amounts.forEach((amount, index) => {
    total += amount * ease(u - index)
  })
  return total
}

/** Share of the plot height above the zero line: in's share of both peaks. */
export function zeroSplit(maxIn: number, maxOut: number): number {
  if (maxIn <= 0 && maxOut <= 0) return RESTING_SPLIT
  const share = maxIn / (Math.max(0, maxIn) + Math.max(0, maxOut))
  return Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, share))
}

/** Maps a series onto canvas points. Pure: it draws nothing and reads no DOM. */
export function lightGeometry(
  series: CashFlowSeries | null,
  { width, height, samples = 140, time = 0, breathe = 0 }: LightLayout,
): LightGeometry {
  const top = height * TOP_CLEARANCE
  const plot = Math.max(0, height - top - BOTTOM_CLEARANCE)
  const buckets = series?.buckets ?? []
  const maxIn = buckets.reduce((peak, b) => Math.max(peak, b.cumulative_income_minor), 0)
  const maxOut = buckets.reduce((peak, b) => Math.max(peak, b.cumulative_expenses_minor), 0)

  const split = zeroSplit(maxIn, maxOut)
  const zeroY = top + plot * split
  // One scale for both sides: whichever would overflow its room sets it.
  const inScale = maxIn > 0 ? (plot * split) / maxIn : Number.POSITIVE_INFINITY
  const outScale = maxOut > 0 ? (plot * (1 - split)) / maxOut : Number.POSITIVE_INFINITY
  const scale = Math.min(inScale, outScale)
  const pixelsPerMinor = Number.isFinite(scale) ? scale : 0

  const count = Math.max(2, Math.round(samples))
  const n = buckets.length
  const wobbleCap = Math.min(2.4, height / 70)

  /**
   * The running total at bucket-space `u`, in O(1): the series already
   * carries each bucket's cumulative total, so a sample needs only its own
   * bucket's amount eased across its width, added to the previous bucket's
   * cumulative total — never a walk over every bucket.
   */
  const sampleAt = (amount: readonly number[], cumulative: readonly number[], u: number): number => {
    if (n === 0) return 0
    const k = Math.min(n - 1, Math.floor(u))
    const frac = u - k
    const before = k > 0 ? (cumulative[k - 1] ?? 0) : 0
    return Math.max(0, before + (amount[k] ?? 0) * ease(frac))
  }

  const edge = (
    amount: readonly number[],
    cumulative: readonly number[],
    direction: -1 | 1,
    phase: number,
  ): LightPoint[] => {
    const points: LightPoint[] = []
    for (let i = 0; i < count; i += 1) {
      const t = i / (count - 1)
      const value = sampleAt(amount, cumulative, t * n)
      const lift = value * pixelsPerMinor
      // The wobble is at most a fifth of a thin band, so it never crosses the line.
      const wobble =
        breathe * Math.sin(i * 0.19 + time * 0.9 + phase) * wobbleCap * Math.min(1, lift / 12)
      points.push({ x: t * width, y: zeroY + direction * Math.max(0, lift + wobble) })
    }
    return points
  }

  return {
    zeroY,
    inEdge: edge(
      buckets.map((b) => b.income_minor),
      buckets.map((b) => b.cumulative_income_minor),
      -1,
      0,
    ),
    outEdge: edge(
      buckets.map((b) => b.expenses_minor),
      buckets.map((b) => b.cumulative_expenses_minor),
      1,
      2.1,
    ),
  }
}

function channels(hex: string): [number, number, number] {
  return [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16)) as [number, number, number]
}

/** The colour `t` of the way along a three-stop gradient (stops at 0, 0.5 and 1). */
export function ledgerColourAt(stops: LedgerStops, t: number, alpha = 1): string {
  const u = Math.min(1, Math.max(0, t))
  const [from, to, k] = u <= 0.5 ? [stops[0], stops[1], u * 2] : [stops[1], stops[2], (u - 0.5) * 2]
  const a = channels(from)
  const b = channels(to)
  const mix = a.map((value, i) => Math.round(value + ((b[i] ?? value) - value) * k))
  return `rgba(${mix.join(',')},${alpha})`
}
