import { useCallback, useEffect, useId, useState } from 'react'
import { CalendarDays } from 'lucide-react'
import { cn } from '../lib/cn'
import { formatDate, parseEuropeanDateToISO } from '../lib/money'
import { CalendarPopover, type YearMonth } from './CalendarPopover'
import { Button, Input } from './ui'
import { useI18n } from '../lib/I18nProvider'

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
  // State, not a ref: the calendar is drawn from the field's element.
  const [field, setField] = useState<HTMLDivElement | null>(null)
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

  const closeCalendar = useCallback(() => setOpen(false), [])

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

  function pick(iso: string) {
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

  return (
    <div ref={setField}>
      <div className="relative">
        <Input
          value={text}
          onChange={(e) => {
            setText(e.target.value)
            setInvalid(false)
            // Typing is the other way to give the date, and what it leads to
            // (the message below, a new month) would move under the calendar.
            setOpen(false)
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

      {open && !disabled && field ? (
        <CalendarPopover
          field={field}
          value={value}
          view={view}
          onShiftMonth={shiftMonth}
          onPick={pick}
          onClose={closeCalendar}
        />
      ) : null}
    </div>
  )
}
