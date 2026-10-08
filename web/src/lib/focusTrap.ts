/** Elements a Tab press can land on. */
const FOCUSABLE = 'a[href],button,input,select,textarea,[tabindex]:not([tabindex="-1"])'

/** Returns the controls in `root` that a Tab press can land on, in document order. */
export function tabStops(root: ParentNode): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => !el.hasAttribute('disabled') && el.getAttribute('aria-hidden') !== 'true',
  )
}

/**
 * Returns the control a Tab press must jump to so that focus wraps instead of
 * leaving the window, or null when the browser's own move stays inside.
 *
 * The quick-add panel hides when it loses focus; a Tab past its last control
 * would hand focus to the main window and the keystrokes after it with it.
 */
export function wrapTarget(
  stops: readonly HTMLElement[],
  active: Element | null,
  backwards: boolean,
): HTMLElement | null {
  const first = stops[0]
  const last = stops[stops.length - 1]
  if (!first || !last) return null

  // Focus on the body or outside the list counts as being past the end.
  const inside = active !== null && stops.includes(active as HTMLElement)
  if (backwards) return !inside || active === first ? last : null
  return !inside || active === last ? first : null
}

/** Keydown handler that keeps Tab inside `root`. */
export function trapTab(root: ParentNode, event: KeyboardEvent): void {
  if (event.key !== 'Tab') return

  const target = wrapTarget(tabStops(root), document.activeElement, event.shiftKey)
  if (!target) return

  event.preventDefault()
  target.focus()
}
