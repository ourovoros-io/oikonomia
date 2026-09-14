import { useEffect, useRef, useState } from 'react'
import { CalendarDays, ChevronLeft, ChevronRight } from 'lucide-react'
import { cn } from '../lib/cn'
import { daysInMonth, formatDate, parseEuropeanDateToISO } from '../lib/money'
import { Button, Input } from './ui'
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
      if (rootRef.current && e.target instanceof Node && !rootRef.current.contains(e.target)) {
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', onDown)
    return () => document.removeEventListener('mousedown', onDown)
  }, [open, disabled])

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
      setInvalid(true)
    }
  }

  function pick(day: number) {
    const iso = toIso(view.y, view.m, day)
    setInvalid(false)
    setOpen(false)
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
    <div
      ref={rootRef}
      className="relative"
      onKeyDown={(e) => {
        if (e.key === 'Escape' && open) {
          e.stopPropagation()
          setOpen(false)
        }
      }}
    >
      <Input
        value={text}
        onChange={(e) => setText(e.target.value)}
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
        className={cn(
          'pr-11',
          invalid &&
            'border-[var(--color-danger)] focus:border-[var(--color-danger)] focus:ring-[var(--color-danger)]/25',
        )}
      />
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="absolute top-1/2 right-1 h-8 w-8 -translate-y-1/2"
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

      {open && !disabled ? (
        <div className="glass-dialog absolute top-full left-0 z-30 mt-2 w-64 rounded-[16px] p-3">
          <div className="mb-2 flex items-center justify-between">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-7 w-7"
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
              size="icon"
              className="h-7 w-7"
              onClick={() => shiftMonth(1)}
              aria-label={t('date.nextMonth')}
            >
              <ChevronRight className="size-4" />
            </Button>
          </div>

          <div className="grid grid-cols-7 gap-0.5 text-center">
            {WEEKDAY_KEYS.map((key) => (
              <span key={key} className="py-1 text-[10px] font-medium text-[var(--color-muted)]">
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
        </div>
      ) : null}
    </div>
  )
}
