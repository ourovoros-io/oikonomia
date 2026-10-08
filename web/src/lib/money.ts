import { getLocale } from './i18n'

/**
 * A currency as core describes it: its code, and how many decimals one minor
 * unit is.
 *
 * Amounts are stored as integer minor units, and core alone decides what a
 * minor unit of a currency is (`currency_minor_exponent` in oikonomia-core).
 * Every conversion between minor units and a displayed or typed amount takes
 * `decimals` from here. The webview's own `Intl` data may give a currency
 * another number of decimals than core does, and reading it would show a book
 * 100 or 1000 times off from what is stored; `moneyDecimals.test.ts` guards
 * against that.
 */
export type Currency = {
  /** Three capital letters, such as `EUR`. */
  readonly code: string
  /** Decimals of one minor unit, as core sent them. */
  readonly decimals: number
}

/** The largest number of decimals `Intl.NumberFormat` and `toFixed` accept. */
const MAX_DECIMALS = 20

/**
 * The currency of a book, from the entity core sent.
 *
 * Throws when the entity carries no usable number of decimals: showing or
 * storing amounts at a guessed scale would be worse than failing.
 */
export function bookCurrency(entity: {
  base_currency: string
  base_currency_decimals: number
}): Currency {
  const decimals = entity.base_currency_decimals

  if (!Number.isInteger(decimals) || decimals < 0 || decimals > MAX_DECIMALS) {
    throw new RangeError(
      `Book currency ${entity.base_currency}: base_currency_decimals is ${String(decimals)}, ` +
        `expected an integer from 0 to ${MAX_DECIMALS}. The entity must come from core's entity commands.`,
    )
  }

  return { code: entity.base_currency, decimals }
}

/**
 * The amount as plain text for an amount input: digits and the decimal mark
 * of the app language (`1234.50`, `1234,50`), with exactly the currency's
 * decimals and no grouping, so the form shows what the list shows.
 * Built from the digits of the integer, so no rounding is involved.
 */
export function minorToInputText(
  minor: number,
  currency: Currency,
  locale: string = getLocale(),
): string {
  const digits = String(Math.abs(Math.trunc(minor))).padStart(currency.decimals + 1, '0')
  const whole = digits.slice(0, digits.length - currency.decimals)
  const fraction = digits.slice(digits.length - currency.decimals)
  const sign = minor < 0 ? '-' : ''

  return fraction ? `${sign}${whole}${decimalMark(locale)}${fraction}` : `${sign}${whole}`
}

/**
 * The decimal mark of `locale`, which is what the app language writes: `.` in
 * English, `,` in Greek, French and German.
 */
function decimalMark(locale: string): string {
  const mark = new Intl.NumberFormat(locale)
    .formatToParts(1.1)
    .find((part) => part.type === 'decimal')

  return mark?.value ?? '.'
}

/**
 * Format minor units as currency for display, in the app language.
 *
 * `Intl.NumberFormat` supplies the grouping, the decimal mark and the
 * currency sign of the locale; `locale` defaults to the app language, so one
 * book reads the same on every screen and in the report PDF. The number of
 * decimals is always `currency.decimals`, set as both the minimum and the
 * maximum so the locale data's own figure for the currency is never used.
 */
export function formatMoney(
  minor: number,
  currency: Currency,
  locale: string = getLocale(),
  opts?: { signed?: boolean },
): string {
  const absolute = Math.abs(minor) / 10 ** currency.decimals
  const formatted = formatAbsolute(absolute, currency, locale)

  if (minor < 0) return `-${formatted}`
  if (opts?.signed && minor > 0) return `+${formatted}`
  return formatted
}

/** The magnitude as currency text; a code Intl rejects is written as grouped digits plus the code. */
function formatAbsolute(absolute: number, currency: Currency, locale: string): string {
  const digits = currency.decimals

  try {
    return new Intl.NumberFormat(locale, {
      style: 'currency',
      currency: currency.code,
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    }).format(absolute)
  } catch {
    const plain = new Intl.NumberFormat(locale, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    })

    return `${plain.format(absolute)} ${currency.code}`
  }
}

// time crate Month may serialize as string name or number.
const MONTHS: Record<string, number> = {
  January: 1,
  February: 2,
  March: 3,
  April: 4,
  May: 5,
  June: 6,
  July: 7,
  August: 8,
  September: 9,
  October: 10,
  November: 11,
  December: 12,
}

function pad2(n: number): string {
  return String(n).padStart(2, '0')
}

/** Parse a Rust entry date (`YYYY-MM-DD` string or `{ year, month, day }`). */
function dateParts(value: unknown): { y: number; m: number; d: number } | null {
  if (typeof value === 'string') {
    const iso = /^(\d{4})-(\d{2})-(\d{2})/.exec(value)
    if (!iso) return null
    return { y: Number(iso[1]), m: Number(iso[2]), d: Number(iso[3]) }
  }
  if (value && typeof value === 'object') {
    const o = value as Record<string, unknown>
    const y = o.year
    let m: number | null = null
    if (typeof o.month === 'number') m = o.month
    else if (typeof o.month === 'string') m = MONTHS[o.month] ?? null
    const d = o.day
    if (typeof y === 'number' && m != null && typeof d === 'number') {
      return { y, m, d }
    }
  }
  return null
}

/**
 * Render entry dates from Rust the European / Greek way: `dd/mm/yyyy`.
 * Display only — anything sent back to the API stays ISO.
 */
export function formatDate(value: unknown): string {
  const p = dateParts(value)
  if (!p) return typeof value === 'string' ? value : String(value ?? '')
  return `${pad2(p.d)}/${pad2(p.m)}/${p.y}`
}

/** Normalize an entry date to ISO `YYYY-MM-DD` (for date inputs). */
export function isoDate(value: unknown): string {
  const p = dateParts(value)
  if (!p) return typeof value === 'string' ? value : ''
  return `${p.y}-${pad2(p.m)}-${pad2(p.d)}`
}

/** Days in a month (1-based), Gregorian leap rules. */
export function daysInMonth(y: number, m: number): number {
  if (m === 2) {
    const leap = (y % 4 === 0 && y % 100 !== 0) || y % 400 === 0
    return leap ? 29 : 28
  }
  return [4, 6, 9, 11].includes(m) ? 30 : 31
}

/**
 * Parse a user-typed European date — `dd/mm/yyyy`, tolerant of `d.m.yy` and
 * dashes, with ISO accepted as a fallback — into ISO `YYYY-MM-DD`.
 * Returns null when the input is not a real calendar date.
 */
export function parseEuropeanDateToISO(input: string): string | null {
  const s = input.trim()
  const eu = /^(\d{1,2})[./-](\d{1,2})[./-](\d{2}|\d{4})$/.exec(s)
  const iso = /^(\d{4})-(\d{1,2})-(\d{1,2})$/.exec(s)

  let y: number
  let m: number
  let d: number
  if (eu) {
    d = Number(eu[1])
    m = Number(eu[2])
    y = eu[3].length === 2 ? 2000 + Number(eu[3]) : Number(eu[3])
  } else if (iso) {
    y = Number(iso[1])
    m = Number(iso[2])
    d = Number(iso[3])
  } else {
    return null
  }

  if (m < 1 || m > 12 || d < 1 || d > daysInMonth(y, m)) return null
  return `${String(y).padStart(4, '0')}-${pad2(m)}-${pad2(d)}`
}
