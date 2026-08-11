import { useEffect, useRef, type RefObject } from 'react'

const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(', ')

/**
 * Stack of open dialogs, innermost last. Only the top-most dialog reacts to
 * keyboard events, so a nested ConfirmDialog consumes Escape without closing
 * the modal underneath it.
 */
const openDialogs: symbol[] = []

/**
 * Dialog keyboard behavior: focus the panel on open, keep Tab cycling inside
 * it, close the top-most dialog on Escape, and restore focus to the
 * previously focused element on close.
 *
 * The panel element must have `tabIndex={-1}` so it can take initial focus.
 */
export function useDialogFocus(
  panelRef: RefObject<HTMLElement | null>,
  active: boolean,
  onEscape: () => void,
) {
  // Ref so a new onEscape closure per render does not re-run the effect
  // (which would steal focus back to the panel on every render).
  const onEscapeRef = useRef(onEscape)
  onEscapeRef.current = onEscape

  useEffect(() => {
    if (!active) return

    const id = Symbol('dialog')
    openDialogs.push(id)

    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    panelRef.current?.focus()

    const onKeyDown = (e: KeyboardEvent) => {
      if (openDialogs[openDialogs.length - 1] !== id) return
      const panel = panelRef.current
      if (!panel) return

      if (e.key === 'Escape') {
        onEscapeRef.current()
        return
      }
      if (e.key !== 'Tab') return

      // offsetParent is null for display:none subtrees (e.g. hidden file inputs).
      const items = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
        (el) => el.offsetParent !== null,
      )
      if (items.length === 0) {
        e.preventDefault()
        return
      }

      const first = items[0]
      const last = items[items.length - 1]
      const current = document.activeElement

      if (!(current instanceof HTMLElement) || !panel.contains(current)) {
        e.preventDefault()
        first.focus()
      } else if (e.shiftKey && (current === first || current === panel)) {
        e.preventDefault()
        last.focus()
      } else if (!e.shiftKey && current === last) {
        e.preventDefault()
        first.focus()
      }
    }

    document.addEventListener('keydown', onKeyDown)

    return () => {
      document.removeEventListener('keydown', onKeyDown)
      const index = openDialogs.indexOf(id)
      if (index >= 0) openDialogs.splice(index, 1)
      previous?.focus()
    }
  }, [active, panelRef])
}
