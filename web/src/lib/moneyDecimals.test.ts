import { readFileSync } from 'node:fs'
import { describe, expect, test } from 'vitest'
import { productionSources, SRC_ROOT } from '../test/sourceFiles'
import { stripComments } from './stripComments.testutil'

/**
 * Guard: the number of decimals of a currency comes from core, never from the
 * webview.
 *
 * Amounts are integer minor units, and core decides what a minor unit of each
 * currency is. It sends that number with the book (`base_currency_decimals`),
 * and `money.ts` scales every amount by it. The webview has its own figure for
 * each currency in its `Intl` data, and that figure changes between versions
 * of the data: HUF has two decimals in one and none in the next. Money code
 * that reads it shows a book 100 times off from what is stored.
 *
 * Three things are checked:
 * - nothing reads `resolvedOptions()`, which is how the `Intl` figure is read;
 * - only `lib/money.ts` formats with `style: 'currency'`, so every amount on
 *   screen goes through the one function that is given core's number;
 * - a currency formatter sets both `minimumFractionDigits` and
 *   `maximumFractionDigits`, and to no number written in the source, so the
 *   `Intl` default and a hard-coded 2 are both out.
 *
 * This is a tripwire, not a proof. It reads source text, so it cannot see an
 * alias (`const options = { style: kind }`), a formatter built from a spread
 * object, or arithmetic that scales by a literal (`minor / 100`). Code review
 * catches those.
 */

/** The only production file allowed to build a currency formatter. */
const CURRENCY_FORMATTER_ALLOWED = 'lib/money.ts'

const RESOLVED_OPTIONS = /\bresolvedOptions\b/g
const CURRENCY_STYLE = /\bstyle\s*:\s*['"`]currency['"`]/g
const FRACTION_OPTION = /\b(minimum|maximum)FractionDigits\s*:\s*([^,}\s]+)/g

function lineOf(code: string, index: number): number {
  return code.slice(0, index).split('\n').length
}

/** The `{ ... }` around `index`, assuming no braces nest inside it. */
function enclosingObject(code: string, index: number): string {
  const open = code.lastIndexOf('{', index)
  const close = code.indexOf('}', index)

  return code.slice(open, close === -1 ? code.length : close + 1)
}

/** What is wrong with the options of one currency formatter, if anything. */
function formatterFaults(options: string): string[] {
  const faults: string[] = []
  const set = new Map<string, string>()

  for (const match of options.matchAll(FRACTION_OPTION)) set.set(match[1], match[2])

  for (const bound of ['minimum', 'maximum']) {
    const value = set.get(bound)

    if (value === undefined) faults.push(`${bound}FractionDigits left to Intl`)
    else if (/^\d/.test(value)) faults.push(`${bound}FractionDigits hard-coded`)
  }

  return faults
}

/**
 * Every place `source` takes the number of decimals from somewhere other than
 * core, as "line: fault". `mayFormatCurrency` is false for every file but
 * `lib/money.ts`. Pure, so the tests below exercise the same code as the
 * repository scan.
 */
function decimalsFaults(source: string, mayFormatCurrency: boolean): string[] {
  const code = stripComments(source)
  const hits: Array<{ index: number; text: string }> = []

  for (const match of code.matchAll(RESOLVED_OPTIONS)) {
    hits.push({ index: match.index, text: 'Intl resolved options read' })
  }

  for (const match of code.matchAll(CURRENCY_STYLE)) {
    if (!mayFormatCurrency) {
      hits.push({ index: match.index, text: 'currency formatter outside lib/money.ts' })
      continue
    }

    for (const fault of formatterFaults(enclosingObject(code, match.index))) {
      hits.push({ index: match.index, text: fault })
    }
  }

  return hits
    .sort((a, b) => a.index - b.index)
    .map((hit) => `${lineOf(code, hit.index)}: ${hit.text}`)
}

describe('decimalsFaults', () => {
  // [description, source, allowed to format currency, the faults that must fire]
  const flagged: Array<[string, string, boolean, string[]]> = [
    [
      'the Intl figure read through resolved options',
      "const digits = new Intl.NumberFormat('en', options).resolvedOptions().maximumFractionDigits",
      true,
      ['1: Intl resolved options read'],
    ],
    [
      'resolved options split over lines',
      'const options = formatter\n  .resolvedOptions()',
      false,
      ['2: Intl resolved options read'],
    ],
    [
      'a currency formatter in a screen',
      "new Intl.NumberFormat(locale, { style: 'currency', currency: code }).format(amount)",
      false,
      ['1: currency formatter outside lib/money.ts'],
    ],
    [
      'toLocaleString with a currency style in a screen',
      'amount.toLocaleString(locale, { style: "currency", currency: code })',
      false,
      ['1: currency formatter outside lib/money.ts'],
    ],
    [
      'a currency formatter that leaves the decimals to Intl',
      "new Intl.NumberFormat(locale, { style: 'currency', currency: code })",
      true,
      ['1: minimumFractionDigits left to Intl', '1: maximumFractionDigits left to Intl'],
    ],
    [
      'a currency formatter that sets only the maximum',
      "new Intl.NumberFormat(locale, { style: 'currency', currency: code, maximumFractionDigits: digits })",
      true,
      ['1: minimumFractionDigits left to Intl'],
    ],
    [
      'a currency formatter with a hard-coded 2',
      "new Intl.NumberFormat(locale, {\n  style: 'currency',\n  currency: code,\n  minimumFractionDigits: 2,\n  maximumFractionDigits: 2,\n})",
      true,
      ['2: minimumFractionDigits hard-coded', '2: maximumFractionDigits hard-coded'],
    ],
  ]

  test.each(flagged)('flags %s', (_name, source, mayFormatCurrency, faults) => {
    expect(decimalsFaults(source, mayFormatCurrency)).toEqual(faults)
  })

  const allowed: Array<[string, string, boolean]> = [
    [
      'a currency formatter given both bounds from a value',
      "new Intl.NumberFormat(locale, {\n  style: 'currency',\n  currency: code,\n  minimumFractionDigits: digits,\n  maximumFractionDigits: digits,\n})",
      true,
    ],
    [
      'a percentage formatter with its own decimals',
      'new Intl.NumberFormat(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 })',
      false,
    ],
    ['a read inside a comment', '// never call resolvedOptions() here', false],
    ['a currency style named in a comment', "/* not { style: 'currency' } */\nrun()", false],
    ['a date formatter', "date.toLocaleDateString('el-GR', { day: '2-digit' })", false],
  ]

  test.each(allowed)('does not flag %s', (_name, source, mayFormatCurrency) => {
    expect(decimalsFaults(source, mayFormatCurrency)).toEqual([])
  })
})

describe('the number of decimals comes from core', () => {
  test('no production code reads the decimals of a currency from Intl or hard-codes them', () => {
    const offenders: string[] = []

    for (const file of productionSources()) {
      const relative = file.slice(SRC_ROOT.length)
      const faults = decimalsFaults(readFileSync(file, 'utf8'), relative === CURRENCY_FORMATTER_ALLOWED)

      for (const fault of faults) offenders.push(`${relative}:${fault}`)
    }

    expect(offenders).toEqual([])
  })

  test('lib/money.ts still builds the currency formatter the scan checks', () => {
    const source = stripComments(readFileSync(`${SRC_ROOT}${CURRENCY_FORMATTER_ALLOWED}`, 'utf8'))

    expect([...source.matchAll(CURRENCY_STYLE)]).toHaveLength(1)
  })
})
