/** Currency decimal places (ISO 4217 subset). Most are 2; some are 0 or 3. */
export function currencyFractionDigits(currency: string): number {
  try {
    return (
      new Intl.NumberFormat('en', { style: 'currency', currency }).resolvedOptions()
        .maximumFractionDigits ?? 2
    )
  } catch {
    return 2
  }
}

/**
 * Parse a user-entered amount into minor units.
 * Accepts:
 * - `25` / `25.50` (dot decimal)
 * - `25,50` (comma decimal, common in Europe)
 * - `1,234.56` / `1.234,56` (thousands separators)
 */
export function parseMajorToMinor(input: string, currency = 'EUR'): number | null {
  let s = input.trim()
  if (!s) return null

  const neg = s.startsWith('-')
  if (neg) s = s.slice(1).trim()
  // Strip currency symbols and spaces
  s = s.replace(/[^\d.,]/g, '')
  if (!s) return null

  const lastComma = s.lastIndexOf(',')
  const lastDot = s.lastIndexOf('.')

  let normalized: string
  if (lastComma >= 0 && lastDot >= 0) {
    // Both present: the last separator is the decimal mark
    if (lastComma > lastDot) {
      // 1.234,56
      normalized = s.replace(/\./g, '').replace(',', '.')
    } else {
      // 1,234.56
      normalized = s.replace(/,/g, '')
    }
  } else if (lastComma >= 0) {
    // Only commas: if exactly one comma and 1–3 digits after → decimal
    const parts = s.split(',')
    if (parts.length === 2 && parts[1].length > 0 && parts[1].length <= 3) {
      normalized = `${parts[0].replace(/\./g, '')}.${parts[1]}`
    } else {
      // Thousands separators only
      normalized = s.replace(/,/g, '')
    }
  } else if (lastDot >= 0) {
    const parts = s.split('.')
    if (parts.length === 2 && parts[1].length > 0 && parts[1].length <= 3) {
      normalized = s
    } else if (parts.length > 2) {
      // 1.234.567 unlikely as decimals — treat dots as thousands
      normalized = s.replace(/\./g, '')
    } else {
      normalized = s
    }
  } else {
    normalized = s
  }

  if (!/^\d+(\.\d+)?$/.test(normalized)) return null

  const digits = currencyFractionDigits(currency)
  const [whole, frac = ''] = normalized.split('.')
  if (frac.length > digits) return null

  const fracPad = (frac + '0'.repeat(digits)).slice(0, digits)
  const factor = 10 ** digits
  const minor = Number(whole) * factor + (fracPad ? Number(fracPad) : 0)
  if (!Number.isFinite(minor)) return null
  return neg ? -minor : minor
}

/** Prefer a locale that matches the currency for readable dashboards. */
export function localeForCurrency(currency: string): string {
  const c = (currency || 'EUR').toUpperCase()
  if (typeof navigator !== 'undefined') {
    // Keep user locale if it already matches the currency area reasonably.
    const nav = navigator.language || 'en'
    if (c === 'EUR' && (nav.startsWith('el') || nav.startsWith('de') || nav.startsWith('fr'))) {
      return nav
    }
    if (c === 'USD' && nav.startsWith('en')) return nav
    if (c === 'GBP' && nav.startsWith('en')) return 'en-GB'
  }
  switch (c) {
    case 'EUR':
      return 'el-GR'
    case 'GBP':
      return 'en-GB'
    case 'USD':
      return 'en-US'
    default:
      return typeof navigator !== 'undefined' ? navigator.language : 'en'
  }
}

/** Format minor units as currency for display. */
export function formatMoney(
  minor: number,
  currency: string,
  locale?: string,
  opts?: { signed?: boolean },
): string {
  const ccy = currency || 'EUR'
  const loc = locale || localeForCurrency(ccy)
  const digits = currencyFractionDigits(ccy)
  const factor = 10 ** digits
  const major = minor / factor
  try {
    const formatted = new Intl.NumberFormat(loc, {
      style: 'currency',
      currency: ccy,
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    }).format(Math.abs(major))

    // Avoid double signs (some locales already produce a minus).
    if (opts?.signed) {
      if (minor > 0) return `+${formatted}`
      if (minor < 0) return `-${formatted}`
      return formatted
    }
    if (minor < 0) return `-${formatted}`
    return formatted
  } catch {
    const abs = Math.abs(major).toFixed(digits)
    if (opts?.signed && minor > 0) return `+${abs} ${ccy}`
    if (minor < 0) return `-${abs} ${ccy}`
    return `${abs} ${ccy}`
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
