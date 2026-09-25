import type { CashFlowSeries } from './api'

/**
 * Where the Pulse chart puts things. Pure: it draws nothing and reads no DOM.
 * The amounts come from Rust; this only turns them into pixels.
 */

/** One bucket's column. A side with no money has a height of 0 and is not drawn. */
export type PulseBar = {
  index: number
  /** Centre of the column, in CSS pixels from the left. */
  x: number
  inHeight: number
  outHeight: number
}

export type PulseLayout = {
  zeroY: number
  /** Width of one bucket's column. */
  band: number
  /** Width of a drawn bar, never wider than its column. */
  barWidth: number
  left: number
  bars: PulseBar[]
  /** Bucket with the most money in, or null when nothing came in. */
  peakIn: number | null
  /** Bucket with the most money out, or null when nothing went out. */
  peakOut: number | null
}

export type PulseBox = {
  width: number
  height: number
  /** Room kept clear above and below the bars, e.g. for the peak labels. */
  inset?: number
}

/** Where the zero line rests when there is nothing to draw. */
const RESTING_SPLIT = 0.74
const MIN_SPLIT = 0.3
const MAX_SPLIT = 0.8

const SIDE_GUTTER = 4
const MAX_BAR_WIDTH = 12
const BAR_SHARE = 0.56
/** A day with money in it always shows, however small next to payday. */
const MIN_BAR_HEIGHT = 2

/** Share of the plot height above the zero line: in's share of both peaks. */
export function zeroSplit(maxIn: number, maxOut: number): number {
  if (maxIn <= 0 && maxOut <= 0) return RESTING_SPLIT
  const share = maxIn / (Math.max(0, maxIn) + Math.max(0, maxOut))
  return Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, share))
}

/** Index of the largest positive value, or null when there is none. */
function peakIndex(values: readonly number[]): number | null {
  let best: number | null = null
  values.forEach((value, index) => {
    if (value > 0 && (best === null || value > (values[best] ?? 0))) best = index
  })
  return best
}

/**
 * Lays the series out as one bar per bucket: in rises above the zero line,
 * out hangs below it, and both sides share one scale.
 */
export function pulseLayout(
  series: CashFlowSeries | null,
  { width, height, inset = 4 }: PulseBox,
): PulseLayout {
  const buckets = series?.buckets ?? []
  const incomes = buckets.map((bucket) => Math.max(0, bucket.income_minor))
  const expenses = buckets.map((bucket) => Math.max(0, bucket.expenses_minor))
  const maxIn = Math.max(0, ...incomes)
  const maxOut = Math.max(0, ...expenses)

  const plot = Math.max(0, height - 2 * inset)
  const split = zeroSplit(maxIn, maxOut)
  const zeroY = inset + plot * split

  // One scale for both sides: whichever would overflow its room sets it.
  const inScale = maxIn > 0 ? (plot * split) / maxIn : Number.POSITIVE_INFINITY
  const outScale = maxOut > 0 ? (plot * (1 - split)) / maxOut : Number.POSITIVE_INFINITY
  const scale = Math.min(inScale, outScale)
  const pixelsPerMinor = Number.isFinite(scale) ? scale : 0

  const left = SIDE_GUTTER
  const band = buckets.length > 0 ? Math.max(0, width - 2 * SIDE_GUTTER) / buckets.length : 0
  const barWidth = Math.min(MAX_BAR_WIDTH, band, Math.max(2, band * BAR_SHARE))

  const heightOf = (amount: number) =>
    amount > 0 ? Math.max(MIN_BAR_HEIGHT, amount * pixelsPerMinor) : 0

  const bars = buckets.map((_, index) => ({
    index,
    x: left + band * (index + 0.5),
    inHeight: heightOf(incomes[index] ?? 0),
    outHeight: heightOf(expenses[index] ?? 0),
  }))

  return {
    zeroY,
    band,
    barWidth,
    left,
    bars,
    peakIn: peakIndex(incomes),
    peakOut: peakIndex(expenses),
  }
}

/** The bucket whose column holds `x`, clamped to the ends; null for an empty chart. */
export function bucketAt(layout: PulseLayout, x: number): number | null {
  const count = layout.bars.length
  if (count === 0 || layout.band <= 0) return null
  const index = Math.floor((x - layout.left) / layout.band)
  return Math.min(count - 1, Math.max(0, index))
}

/**
 * A bar anchored flat on the zero line with a rounded far end. `direction`
 * -1 rises (money in), 1 hangs (money out).
 */
export function barPath(
  x: number,
  width: number,
  zeroY: number,
  height: number,
  direction: -1 | 1,
): string {
  const x0 = x - width / 2
  const x1 = x + width / 2
  const radius = Math.min(4, width / 2, height)
  const end = zeroY + direction * height
  const shoulder = end - direction * radius

  return [
    `M${x0},${zeroY}`,
    `V${shoulder}`,
    `Q${x0},${end} ${x0 + radius},${end}`,
    `H${x1 - radius}`,
    `Q${x1},${end} ${x1},${shoulder}`,
    `V${zeroY}`,
    'Z',
  ].join('')
}

/** Parses a `YYYY-MM-DD` bucket date as a local calendar day. */
export function bucketDate(iso: string): Date {
  const [year, month, day] = iso.split('-').map(Number)
  return new Date(year ?? 1970, (month ?? 1) - 1, day ?? 1)
}
