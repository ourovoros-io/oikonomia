import type { Currency } from './money'

/**
 * Typed amounts, read by the same rules as a bank CSV.
 *
 * This is a port of `crates/oikonomia-core/src/csv/amount.rs` (its module doc
 * has the grammar). Core reads CSV cells and this reads the amount fields, so
 * the same text gives the same amount in both. `amountVectors.json` holds
 * the cases, and both test suites read it.
 *
 * The one ambiguity a text cannot settle is `1,280`: with a currency that
 * has decimals it could be one and a bit. A last separator followed by
 * exactly three digits is a thousands group, unless the currency has three
 * decimals, and then the whole text must be a strict grouping. For yen,
 * which has no decimals, `1,280` therefore can only mean one thousand two
 * hundred and eighty.
 */

const CURRENCY_SIGNS = new Set(['€', '$', '£', '¥', '₹', '₺', '₩'])
const MINUS_SIGNS = ['-', '−']
const SPACE = ' '
const APOSTROPHE = "'"

type Split = { whole: string; fraction: string }

/**
 * Parse a typed amount into minor units of `currency`, or null when it is
 * not an amount of that currency (empty, malformed, too many decimals, a
 * code of another currency, or beyond what a JS integer holds exactly).
 */
export function parseMajorToMinor(input: string, currency: Currency): number | null {
  const compact = compactAmount(input)
  if (!compact) return null

  const { negative: wrapped, inner } = stripParentheses(compact)
  const unsigned = readSignedBody(withoutCurrencySigns(inner), currency.code)
  if (unsigned === null) return null

  const split = splitDecimal(unsigned.body, currency.decimals)
  if (!split || (!split.whole && !split.fraction)) return null

  const digits = split.whole + split.fraction.padEnd(currency.decimals, '0')
  const magnitude = Number(digits)
  if (!Number.isSafeInteger(magnitude)) return null

  return wrapped || unsigned.negative ? -magnitude : magnitude
}

/** Whitespace goes, except between two digits, where it stays as one grouping space. */
function compactAmount(raw: string): string {
  let compact = ''
  let gapAfterDigit = false

  for (const character of raw) {
    if (/\s/u.test(character)) {
      gapAfterDigit = /\d$/u.test(compact)
      continue
    }
    if (gapAfterDigit && /\d/u.test(character)) compact += SPACE
    gapAfterDigit = false
    // The typographic apostrophe is the Swiss grouping mark too.
    compact += character === '’' ? APOSTROPHE : character
  }
  return compact
}

function stripParentheses(text: string): { negative: boolean; inner: string } {
  if (text.startsWith('(') && text.endsWith(')') && text.length >= 2) {
    return { negative: true, inner: text.slice(1, -1) }
  }
  return { negative: false, inner: text }
}

function withoutCurrencySigns(text: string): string {
  return [...text].filter((character) => !CURRENCY_SIGNS.has(character)).join('')
}

/** The sign and the bare digits and separators, with a currency code dropped on either side of the sign. */
function readSignedBody(text: string, book: string): { negative: boolean; body: string } | null {
  const coded = stripLetterCode(text, book)
  if (coded === null) return null

  const { negative, rest } = stripSign(coded)
  const body = stripLetterCode(rest, book)
  if (!body || !/^[\d.,' ]+$/u.test(body)) return null

  return { negative, body }
}

function stripSign(text: string): { negative: boolean; rest: string } {
  const first = text.charAt(0)
  const last = text.charAt(text.length - 1)

  if (MINUS_SIGNS.includes(first)) return { negative: true, rest: text.slice(1) }
  if (first === '+') return { negative: false, rest: text.slice(1) }
  if (MINUS_SIGNS.includes(last)) return { negative: true, rest: text.slice(0, -1) }
  return { negative: false, rest: text }
}

/**
 * Three capital letters are a currency code and must be the book's; three
 * letters with a lowercase one are a word and are dropped. Null for another
 * currency's code.
 */
function stripLetterCode(text: string, book: string): string | null {
  const isAnotherCurrency = (letters: string) => /^[A-Z]{3}$/u.test(letters) && letters !== book
  let rest = text

  if (/^[A-Za-z]{3}/u.test(rest)) {
    const letters = rest.slice(0, 3)
    if (rest.length === 3) return rest
    if (isAnotherCurrency(letters)) return null
    rest = rest.slice(3)
  }
  if (/[A-Za-z]{3}$/u.test(rest)) {
    const letters = rest.slice(-3)
    return isAnotherCurrency(letters) ? null : rest.slice(0, -3)
  }
  return rest
}

function splitDecimal(body: string, decimals: number): Split | null {
  const hasSpace = body.includes(SPACE)
  const hasApostrophe = body.includes(APOSTROPHE)
  if (hasSpace && hasApostrophe) return null
  if (hasSpace) return splitGroupedBy(body, SPACE, decimals)
  if (hasApostrophe) return splitGroupedBy(body, APOSTROPHE, decimals)

  const markAt = Math.max(body.lastIndexOf('.'), body.lastIndexOf(','))
  if (markAt < 0) return { whole: body, fraction: '' }

  const mark = body.charAt(markAt)
  const integerPart = body.slice(0, markAt)
  const fraction = body.slice(markAt + 1)

  if (fraction.length === 3 && decimals !== 3) {
    const whole = groupedDigits(body, mark)
    return whole === null ? null : { whole, fraction: '' }
  }
  if (fraction.length > decimals) return null

  if (!/[.,]/u.test(integerPart)) return { whole: integerPart, fraction }

  const whole = groupedDigits(integerPart, mark === '.' ? ',' : '.')
  return whole === null ? null : { whole, fraction }
}

/** A space or apostrophe is only a grouping, so the one `.` or `,` is the decimal mark. */
function splitGroupedBy(body: string, separator: string, decimals: number): Split | null {
  const markAt = body.search(/[.,]/u)
  const whole = markAt < 0 ? body : body.slice(0, markAt)
  const fraction = markAt < 0 ? '' : body.slice(markAt + 1)

  if (!/^\d*$/u.test(fraction) || fraction.length > decimals) return null

  const digits = groupedDigits(whole, separator)
  return digits === null ? null : { whole: digits, fraction }
}

/** The digits of a strict thousands grouping: 1-3 digits without a leading zero, then groups of three. */
function groupedDigits(grouped: string, separator: string): string | null {
  const [first = '', ...groups] = grouped.split(separator)

  if (!/^[1-9]\d{0,2}$/u.test(first)) return null
  if (!groups.every((group) => /^\d{3}$/u.test(group))) return null

  return first + groups.join('')
}
