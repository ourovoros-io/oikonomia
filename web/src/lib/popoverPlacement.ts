/** Space between the popover and the field it hangs from. */
const GAP = 8

/** The closest a popover comes to the edge of the window. */
const EDGE = 8

/** Where the field sits in the window, as `getBoundingClientRect` reports it. */
export type AnchorBox = { top: number; bottom: number; left: number }

export type Size = { width: number; height: number }

function clamp(value: number, min: number, max: number): number {
  // A popover larger than the window keeps its top-left corner on screen.
  return Math.max(min, Math.min(value, max))
}

/**
 * The window position of a popover hung from a field: below it, left edges
 * aligned. It goes above when it does not fit below and does fit above, and
 * is pulled back inside the window when it fits on neither side.
 */
export function placePopover(
  anchor: AnchorBox,
  popover: Size,
  viewport: Size,
): { top: number; left: number } {
  const below = anchor.bottom + GAP
  const above = anchor.top - GAP - popover.height

  const fitsBelow = below + popover.height <= viewport.height - EDGE
  const fitsAbove = above >= EDGE
  const top = fitsBelow || !fitsAbove ? below : above

  return {
    top: clamp(top, EDGE, viewport.height - EDGE - popover.height),
    left: clamp(anchor.left, EDGE, viewport.width - EDGE - popover.width),
  }
}
