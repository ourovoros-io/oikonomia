import { formatDate, type RecurringCadence } from '../lib/api'
import { maxDayOfMonth } from '../lib/recurring'
import { useI18n } from '../lib/I18nProvider'
import { Field, Select } from './ui'

/** Monday first, as the week reads in every language the app is in. */
const WEEKDAYS_MONDAY_FIRST = [1, 2, 3, 4, 5, 6, 0] as const

type Props = {
  cadence: RecurringCadence
  /** Day of the month of a monthly template, as the select's text. */
  dayOfMonth: string
  /** Weekday of a weekly template, 0 for Sunday, as the select's text. */
  weekday: string
  /** Month (1 to 12) and day of a yearly template, as the selects' text. */
  yearMonth: string
  yearDay: string
  /** The date the template starts on, which the pickers decide. */
  nextDate: string
  onDayOfMonth: (value: string) => void
  onWeekday: (value: string) => void
  onYearly: (month: string, day: string) => void
}

/** The part of the recurring form that depends on how often the template recurs. */
export function RecurringCadenceFields({
  cadence,
  dayOfMonth,
  weekday,
  yearMonth,
  yearDay,
  nextDate,
  onDayOfMonth,
  onWeekday,
  onYearly,
}: Props) {
  const { t, locale } = useI18n()
  const weekdayName = new Intl.DateTimeFormat(locale, { weekday: 'long', timeZone: 'UTC' })
  // 2024-01-01 is a Monday, so index 0 is Monday and 6 is Sunday.
  const nameOf = (day: number) =>
    weekdayName.format(new Date(Date.UTC(2024, 0, 1 + ((day + 6) % 7))))
  const daysInMonth = maxDayOfMonth(Number(yearMonth))

  return (
    <>
      {cadence === 'monthly' ? (
        <>
          <Field label={t('recurring.form.dayOfMonth')}>
            <Select
              value={dayOfMonth}
              onChange={(e) => onDayOfMonth(e.target.value)}
              aria-label={t('recurring.form.dayOfMonth')}
            >
              {Array.from({ length: 31 }, (_, i) => String(i + 1)).map((day) => (
                <option key={day} value={day}>
                  {day}
                </option>
              ))}
            </Select>
          </Field>
          {/* col-span-2: this used to sit inside the Field's own grid
              cell; as a sibling now, without the span it would take the
              next column instead of running under it. */}
          <p className="-mt-2 text-xs text-[var(--color-muted)] sm:col-span-2">
            {t('recurring.form.dayOfMonthHint')}
          </p>
        </>
      ) : null}

      {cadence === 'weekly' ? (
        <Field label={t('recurring.form.dayOfWeek')}>
          <Select
            value={weekday}
            onChange={(e) => onWeekday(e.target.value)}
            aria-label={t('recurring.form.dayOfWeek')}
          >
            {WEEKDAYS_MONDAY_FIRST.map((day) => (
              <option key={day} value={String(day)}>
                {nameOf(day)}
              </option>
            ))}
          </Select>
        </Field>
      ) : null}

      {cadence === 'yearly' ? (
        <>
          <Field label={t('recurring.form.month')}>
            <Select
              value={yearMonth}
              onChange={(e) => onYearly(e.target.value, yearDay)}
              aria-label={t('recurring.form.month')}
            >
              {Array.from({ length: 12 }, (_, i) => i + 1).map((month) => (
                <option key={month} value={String(month)}>
                  {t(`date.month.${month}`)}
                </option>
              ))}
            </Select>
          </Field>
          <Field label={t('recurring.form.dayOfMonth')}>
            <Select
              value={yearDay}
              onChange={(e) => onYearly(yearMonth, e.target.value)}
              aria-label={t('recurring.form.dayOfMonth')}
            >
              {Array.from({ length: daysInMonth }, (_, i) => String(i + 1)).map((day) => (
                <option key={day} value={day}>
                  {day}
                </option>
              ))}
            </Select>
          </Field>
        </>
      ) : null}

      <p className="text-xs text-[var(--color-muted)] sm:col-span-2">
        {t('recurring.form.nextDate', { date: formatDate(nextDate) })}
      </p>
    </>
  )
}
