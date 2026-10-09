import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react'
import { createPortal } from 'react-dom'
import { CalendarDays, ChevronLeft, ChevronRight } from 'lucide-react'
import { cn } from '../lib/cn'
import { daysInMonth, formatDate, parseEuropeanDateToISO } from '../lib/money'
import { placePopover } from '../lib/popoverPlacement'
import { Button, Input } from './ui'
import { useI18n } from '../lib/I18nProvider'
import { trapTab } from './useDialogFocus'

const WEEKDAY_KEYS = [
  'date.weekday.mo',
  'date.weekday.tu',
  'date.weekday.we',
  'date.weekday.th',
  'date.weekday.fr',
  'date.weekday.sa',
  'date.weekday.su',
] as const

type YearMonth = { y: number; m: number }

function toIso(y: number, m: number, d: number): string {
  return `${y}-${String(m).padStart(2, '0')}-${String(d).padStart(2, '0')}`
}

function todayIso(): string {
  const now = new Date()
  return toIso(now.getFullYear(), now.getMonth() + 1, now.getDate())
}

function viewFromIso(iso: string): YearMonth {
  const m = /^(\d{4})-(\d{2})/.exec(iso)
  if (m) return { y: Number(m[1]), m: Number(m[2]) }
  const now = new Date()
  return { y: now.getFullYear(), m: now.getMonth() + 1 }
}

type Props = {
  /** ISO `YYYY-MM-DD`, or empty for "no date". */
  value: string
  onChange: (iso: string) => void
  required?: boolean
  disabled?: boolean
  'aria-label'?: string
}

/**
 * European date field: types as dd/mm/yyyy with a Monday-first calendar
 * popover. Native date inputs order their segments by OS region and cannot be
 * forced to day-month-year, so this control replaces them.
 */
export function DateInput({
  value,
  onChange,
  required,
  disabled = false,
  'aria-label': ariaLabel,
}: Props) {
  const { t } = useI18n()
  const [text, setText] = useState(value ? formatDate(value) : '')
  const [invalid, setInvalid] = useState(false)
  const [open, setOpen] = useState(false)
  const [view, setView] = useState<YearMonth>(() => viewFromIso(value))
  const rootRef = useRef<HTMLDivElement>(null)
  const calendarRef = useRef<HTMLDivElement>(null)
  const messageId = useId()

  // The parent commits values only through onChange, so an external value
  // change (prefill, reset) can safely overwrite the draft text.
  useEffect(() => {
    setText(value ? formatDate(value) : '')
    setInvalid(false)
    setView(viewFromIso(value))
  }, [value])

  useEffect(() => {
    if (disabled) setOpen(false)
  }, [disabled])

  useEffect(() => {
    if (!open || disabled) return

    const onDown = (e: MouseEvent) => {
      if (!(e.target instanceof Node)) return
      const inside = rootRef.current?.contains(e.target) || calendarRef.current?.contains(e.target)
      if (!inside) setOpen(false)
    }
    // The calendar is placed once, in the window. When the field moves under
    // it, it closes; following the field would leave it floating over the page
    // once the field has scrolled out of sight.
    const onScroll = (e: Event) => {
      if (e.target instanceof Node && e.target.contains(rootRef.current)) setOpen(false)
    }
    const onResize = () => setOpen(false)

    document.addEventListener('mousedown', onDown)
    // Scroll does not bubble; capture sees it from every scrolling pane.
    window.addEventListener('scroll', onScroll, true)
    window.addEventListener('resize', onResize)
    return () => {
      document.removeEventListener('mousedown', onDown)
      window.removeEventListener('scroll', onScroll, true)
      window.removeEventListener('resize', onResize)
    }
  }, [open, disabled])

  // Placed before paint, and again when the month changes: a month of six
  // weeks is taller than one of five, which matters to a calendar drawn above
  // its field. Written to the element, as the position is layout, not state.
  useLayoutEffect(() => {
    const calendar = calendarRef.current
    // Hung from the whole field box where there is one: a calendar drawn above
    // the bare input would cover the field's label.
    const anchor = rootRef.current?.closest('.field-box') ?? rootRef.current
    if (!open || !anchor || !calendar) return

    const place = placePopover(
      anchor.getBoundingClientRect(),
      { width: calendar.offsetWidth, height: calendar.offsetHeight },
      { width: window.innerWidth, height: window.innerHeight },
    )
    calendar.style.top = `${place.top}px`
    calendar.style.left = `${place.left}px`
  }, [open, disabled, view])

  function toggleButton(): HTMLButtonElement | null {
    return rootRef.current?.querySelector<HTMLButtonElement>('button[aria-expanded]') ?? null
  }

  /** Closes the calendar; focus inside it goes back to its button, not to nowhere. */
  function closeCalendar() {
    if (calendarRef.current?.contains(document.activeElement)) toggleButton()?.focus()
    setOpen(false)
  }

  // The calendar is drawn at the end of the document, so the browser's own Tab
  // order does not lead from the button into it. Each key handled here is kept
  // from a dialog underneath, which would close on Escape or pull Tab back.
  function onKeyDown(e: KeyboardEvent) {
    const calendar = calendarRef.current
    if (!open || !calendar) return

    if (e.key === 'Escape') {
      e.stopPropagation()
      closeCalendar()
      return
    }
    if (e.key !== 'Tab') return

    const insideCalendar = calendar.contains(document.activeElement)
    const entering = e.target === toggleButton() && !e.shiftKey
    if (!insideCalendar && !entering) return
    e.stopPropagation()
    trapTab(e.nativeEvent, calendar)
  }

  function commit() {
    const trimmed = text.trim()
    if (!trimmed) {
      setInvalid(false)
      if (value) onChange('')
      return
    }
    const iso = parseEuropeanDateToISO(trimmed)
    if (iso) {
      setInvalid(false)
      setText(formatDate(iso))
      if (iso !== value) onChange(iso)
    } else {
      // An unreadable date is cleared, not left as the previous value: the
      // form would otherwise save a date the user did not type.
      setInvalid(true)
      if (value) onChange('')
    }
  }

  // A date typed in full counts at once, so a From after To is reported while
  // the person is still in the field, not only after they leave it. A partial
  // date waits for blur: "01/10/20" must not be taken as the year 2020.
  function commitWhenComplete(draft: string) {
    if (!/^\d{1,2}[./-]\d{1,2}[./-]\d{4}$/.test(draft.trim())) return
    const iso = parseEuropeanDateToISO(draft)
    if (iso && iso !== value) onChange(iso)
  }

  function pick(day: number) {
    const iso = toIso(view.y, view.m, day)
    setInvalid(false)
    closeCalendar()
    if (iso !== value) onChange(iso)
    else setText(formatDate(iso))
  }

  function shiftMonth(delta: number) {
    setView((v) => {
      const zero = v.y * 12 + (v.m - 1) + delta
      return { y: Math.floor(zero / 12), m: (zero % 12) + 1 }
    })
  }

  const offset = (new Date(view.y, view.m - 1, 1).getDay() + 6) % 7
  const days = daysInMonth(view.y, view.m)
  const today = todayIso()

  return (
    <div ref={rootRef} onKeyDown={onKeyDown}>
      <div className="relative">
        <Input
          value={text}
          onChange={(e) => {
            setText(e.target.value)
            setInvalid(false)
            commitWhenComplete(e.target.value)
          }}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              commit()
            }
          }}
          placeholder={t('date.placeholder')}
          inputMode="numeric"
          required={required}
          disabled={disabled}
          aria-label={ariaLabel}
          aria-invalid={invalid || undefined}
          aria-describedby={invalid ? messageId : undefined}
          className={cn(
            'pr-11',
            invalid &&
              'border-[var(--color-danger)] focus:border-[var(--color-danger)] focus:ring-[var(--color-danger)]/25',
          )}
        />
        <Button
          type="button"
          variant="ghost"
          size="iconSm"
          className="absolute top-1/2 right-1 -translate-y-1/2"
          onClick={() => {
            if (disabled) return
            setOpen((v) => !v)
          }}
          disabled={disabled}
          aria-label={t('date.openCalendar')}
          aria-expanded={open}
          title={t('date.calendar')}
        >
          <CalendarDays className="size-4" />
        </Button>
      </div>
      {invalid ? (
        <p id={messageId} role="alert" className="mt-1 text-xs text-[var(--color-danger)]">
          {t('date.invalid')}
        </p>
      ) : null}

      {/* On document.body, above every dialog: inside a pane or a dialog the
          calendar is cut off wherever that container clips or scrolls. React
          still bubbles its events to this field, so a dialog around the field
          treats a click on a day as a click inside itself. */}
      {open && !disabled
        ? createPortal(
            <div
              ref={calendarRef}
              className="glass-dialog fixed top-0 left-0 z-[60] w-64 rounded-[16px] p-3"
            >
              <div className="mb-2 flex items-center justify-between">
                <Button
                  type="button"
                  variant="ghost"
                  size="iconSm"
                  onClick={() => shiftMonth(-1)}
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
                  onClick={() => shiftMonth(1)}
                  aria-label={t('date.nextMonth')}
                >
                  <ChevronRight className="size-4" />
                </Button>
              </div>

              <div className="grid grid-cols-7 gap-0.5 text-center">
                {WEEKDAY_KEYS.map((key) => (
                  <span
                    key={key}
                    className="py-1 text-[11px] font-medium text-[var(--color-muted)]"
                  >
                    {t(key)}
                  </span>
                ))}
                {Array.from({ length: offset }, (_, i) => (
                  <span key={`pad-${i}`} />
                ))}
                {Array.from({ length: days }, (_, i) => {
                  const day = i + 1
                  const iso = toIso(view.y, view.m, day)
                  const selected = iso === value
                  return (
                    <button
                      key={day}
                      type="button"
                      onClick={() => pick(day)}
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
              </div>
            </div>,
            document.body,
          )
        : null}
    </div>
  )
}
