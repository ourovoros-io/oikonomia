import { useEffect, useLayoutEffect, useRef } from 'react'
import { createPortal } from 'react-dom'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { todayISO } from '../lib/api'
import { cn } from '../lib/cn'
import { daysInMonth } from '../lib/money'
import { placePopover } from '../lib/popoverPlacement'
import { Button } from './ui'
import { useI18n } from '../lib/I18nProvider'

const WEEKDAY_KEYS = [
  'date.weekday.mo',
  'date.weekday.tu',
  'date.weekday.we',
  'date.weekday.th',
  'date.weekday.fr',
  'date.weekday.sa',
  'date.weekday.su',
] as const

// Six weeks, the most a month spans. Every month is drawn this tall, so the
// calendar keeps its size and its place while the months are paged through:
// the arrows and the days stay under the pointer.
const GRID_CELLS = 42

export type YearMonth = { y: number; m: number }

function toIso(y: number, m: number, d: number): string {
  return `${y}-${String(m).padStart(2, '0')}-${String(d).padStart(2, '0')}`
}

/** Moves focus one control on, or back, inside `calendar`, wrapping at the ends. */
function stepFocus(calendar: HTMLElement, backwards: boolean) {
  const controls = Array.from(calendar.querySelectorAll<HTMLElement>('button:not([disabled])'))
  const at = controls.findIndex((control) => control === document.activeElement)
  const first = backwards ? controls.length - 1 : 0
  const next = at === -1 ? first : (at + (backwards ? -1 : 1) + controls.length) % controls.length
  controls[next]?.focus()
}

function toggleOf(field: HTMLElement): HTMLElement | null {
  return field.querySelector<HTMLElement>('button[aria-expanded]')
}

/** Before the calendar goes: focus inside it returns to its button, not to nowhere. */
function handFocusBack(calendar: HTMLElement | null, field: HTMLElement) {
  if (calendar?.contains(document.activeElement)) toggleOf(field)?.focus()
}

type Props = {
  /** The date field the calendar belongs to: it is placed from it and hands focus back to it. */
  field: HTMLElement
  /** The chosen date, ISO `YYYY-MM-DD`, or empty. */
  value: string
  view: YearMonth
  onShiftMonth: (delta: number) => void
  /** Called with the day chosen, ISO `YYYY-MM-DD`. */
  onPick: (iso: string) => void
  onClose: () => void
}

/**
 * The month calendar of a date field, floating over the page. It is drawn
 * outside the field, in the dialog the field is in or else on the page: inside
 * a pane or a dialog body it is cut off wherever that container clips or
 * scrolls. React still bubbles its events to the field, so a dialog around the
 * field treats a click on a day as a click inside itself.
 */
export function CalendarPopover({ field, value, view, onShiftMonth, onPick, onClose }: Props) {
  const { t } = useI18n()
  const calendarRef = useRef<HTMLDivElement>(null)

  // Inside a modal dialog, everything outside it is inert to assistive
  // technology, so the calendar of a field in a dialog stays in that dialog.
  const host = field.closest('[aria-modal="true"]') ?? document.body

  // Placed once, before paint. Written to the element: the position is
  // layout, not state.
  useLayoutEffect(() => {
    const calendar = calendarRef.current
    if (!calendar) return
    // Hung from the whole field box where there is one: a calendar drawn above
    // the bare input would cover the field's label.
    const anchor = field.closest('.field-box') ?? field

    const place = placePopover(
      anchor.getBoundingClientRect(),
      { width: calendar.offsetWidth, height: calendar.offsetHeight },
      { width: window.innerWidth, height: window.innerHeight },
    )
    calendar.style.top = `${place.top}px`
    calendar.style.left = `${place.left}px`
  }, [field])

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (!(e.target instanceof Node)) return
      if (!field.contains(e.target) && !calendarRef.current?.contains(e.target)) onClose()
    }
    // The calendar does not follow its field. When the field moves under it,
    // it closes: following would leave it floating over the page once the
    // field has scrolled out of sight.
    const onScroll = (e: Event) => {
      if (e.target instanceof Node && e.target.contains(field)) onClose()
    }
    // On the document, before anything else sees the key: WebKit does not
    // focus a button that is clicked, so after a click on the calendar the
    // focus is often nowhere and a handler on the field would not hear it. A
    // key handled here is kept from a dialog underneath, which would close on
    // Escape or pull Tab back to its own first control.
    const onKeyDown = (e: KeyboardEvent) => {
      const calendar = calendarRef.current
      if (!calendar) return

      if (e.key === 'Escape') {
        e.stopPropagation()
        handFocusBack(calendar, field)
        onClose()
        return
      }
      if (e.key !== 'Tab') return

      // The calendar is not next to its button in the document, so Tab is
      // walked by hand: into it from the button, or from nowhere, and round
      // inside it. Tab elsewhere in the field keeps its usual meaning.
      const focused = document.activeElement
      const inside = calendar.contains(focused)
      const entering = focused === toggleOf(field) && !e.shiftKey
      const nowhere = focused === null || focused === document.body
      if (!inside && !entering && !nowhere) return
      e.preventDefault()
      e.stopPropagation()
      stepFocus(calendar, e.shiftKey)
    }

    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKeyDown, true)
    // Scroll does not bubble; capture sees it from every scrolling pane.
    window.addEventListener('scroll', onScroll, true)
    window.addEventListener('resize', onClose)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKeyDown, true)
      window.removeEventListener('scroll', onScroll, true)
      window.removeEventListener('resize', onClose)
    }
  }, [field, onClose])

  function pick(iso: string) {
    handFocusBack(calendarRef.current, field)
    onPick(iso)
  }

  const offset = (new Date(view.y, view.m - 1, 1).getDay() + 6) % 7
  const days = daysInMonth(view.y, view.m)
  const today = todayISO()

  return createPortal(
    <div
      ref={calendarRef}
      role="dialog"
      aria-label={t('date.calendar')}
      className="glass-dialog fixed top-0 left-0 z-[60] w-64 rounded-[16px] p-3"
    >
      <div className="mb-2 flex items-center justify-between">
        <Button
          type="button"
          variant="ghost"
          size="iconSm"
          onClick={() => onShiftMonth(-1)}
          aria-label={t('date.prevMonth')}
        >
          <ChevronLeft className="size-4" />
        </Button>
        <span className="text-sm font-medium text-[var(--color-fg)]">
          {t(`date.month.${view.m}`)} {view.y}
        </span>
        <Button
          type="button"
          variant="ghost"
          size="iconSm"
          onClick={() => onShiftMonth(1)}
          aria-label={t('date.nextMonth')}
        >
          <ChevronRight className="size-4" />
        </Button>
      </div>

      <div className="grid grid-cols-7 gap-0.5 text-center">
        {WEEKDAY_KEYS.map((key) => (
          <span key={key} className="py-1 text-[11px] font-medium text-[var(--color-muted)]">
            {t(key)}
          </span>
        ))}
        {Array.from({ length: offset }, (_, i) => (
          <span key={`before-${i}`} />
        ))}
        {Array.from({ length: days }, (_, i) => {
          const day = i + 1
          const iso = toIso(view.y, view.m, day)
          const selected = iso === value
          return (
            <button
              key={day}
              type="button"
              onClick={() => pick(iso)}
              className={cn(
                'h-8 rounded-lg text-sm tabular-nums transition',
                selected
                  ? 'bg-[var(--color-accent)] font-semibold text-white'
                  : 'text-[var(--color-fg-secondary)] hover:bg-[var(--color-surface-elevated)] hover:text-[var(--color-fg)]',
                !selected && iso === today && 'ring-1 ring-[var(--color-accent)]/50',
              )}
            >
              {day}
            </button>
          )
        })}
        {Array.from({ length: GRID_CELLS - offset - days }, (_, i) => (
          <span key={`after-${i}`} className="h-8" />
        ))}
      </div>
    </div>,
    host,
  )
}
