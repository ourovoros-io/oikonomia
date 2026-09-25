/** The meter starts at the lower left (135 degrees, SVG angles run clockwise). */
export const ARC_START_DEG = 135
/** It sweeps three quarters of a turn, leaving the gap at the bottom. */
export const ARC_SWEEP_DEG = 270

/**
 * How much of the arc to light for a value in basis points: 0 to 1, clamped.
 * `null` when there is no value, so the caller draws an empty arc, never 0%.
 */
export function arcFraction(bps: number | null | undefined, fullScaleBps = 10_000): number | null {
  if (bps === null || bps === undefined || !Number.isFinite(bps)) return null
  return Math.min(1, Math.max(0, bps / fullScaleBps))
}

export function arcEndDeg(fraction: number): number {
  return ARC_START_DEG + ARC_SWEEP_DEG * fraction
}

export function arcPoint(cx: number, cy: number, radius: number, deg: number): { x: number; y: number } {
  const rad = (deg * Math.PI) / 180
  return { x: cx + radius * Math.cos(rad), y: cy + radius * Math.sin(rad) }
}

/** An SVG path along the circle from `fromDeg` to `toDeg`, clockwise. */
export function arcPath(cx: number, cy: number, radius: number, fromDeg: number, toDeg: number): string {
  const start = arcPoint(cx, cy, radius, fromDeg)
  const end = arcPoint(cx, cy, radius, toDeg)
  const largeArc = toDeg - fromDeg > 180 ? 1 : 0
  return `M${start.x.toFixed(2)} ${start.y.toFixed(2)}A${radius} ${radius} 0 ${largeArc} 1 ${end.x.toFixed(2)} ${end.y.toFixed(2)}`
}

/** A basis-point value as a percent with one decimal, in the reader's locale. */
export function formatPercentFromBps(
  bps: number,
  locale: string,
  options: { signed?: boolean } = {},
): string {
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
    signDisplay: options.signed ? 'exceptZero' : 'auto',
  }).format(bps / 100)
}
