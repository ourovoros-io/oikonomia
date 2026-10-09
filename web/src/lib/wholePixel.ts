/**
 * How far to shift an element whose top sits on a fractional CSS pixel.
 *
 * `m-auto` splits leftover space in half, and a fractional leftover lands
 * the unlock card on a sub-pixel (measured at 290.297). Moving by less than
 * half a pixel reaches the nearer whole pixel without a visible jump.
 * A non-finite top is left alone.
 */
export function wholePixelShift(top: number): number {
  if (!Number.isFinite(top)) return 0

  const fraction = top - Math.floor(top)
  if (fraction === 0) return 0

  return fraction < 0.5 ? -fraction : 1 - fraction
}
