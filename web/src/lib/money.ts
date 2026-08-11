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

/**
 * Render entry dates from Rust (`YYYY-MM-DD` or `{ year, month, day }`) the
 * European / Greek way: `dd/mm/yyyy`. Display only — anything sent back to
 * the API stays ISO.
 */
export function formatDate(value: unknown): string {
  if (typeof value === 'string') {
    const iso = /^(\d{4})-(\d{2})-(\d{2})/.exec(value)
    if (iso) return `${iso[3]}/${iso[2]}/${iso[1]}`
    return value
  }
  if (value && typeof value === 'object') {
    const o = value as Record<string, unknown>
    const y = o.year
    // time crate Month may serialize as string name or number
    let m: number | null = null
    if (typeof o.month === 'number') m = o.month
    else if (typeof o.month === 'string') {
      const map: Record<string, number> = {
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
      m = map[o.month] ?? null
    }
    const d = o.day
    if (typeof y === 'number' && m != null && typeof d === 'number') {
      return `${String(d).padStart(2, '0')}/${String(m).padStart(2, '0')}/${y}`
    }
  }
  return String(value ?? '')
}
