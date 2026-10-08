import { afterEach, describe, expect, test } from 'vitest'
import { parseMajorToMinor } from './amountParse'
import {
  amountProblemKey,
  bookCurrency,
  formatDate,
  formatMoney,
  isoDate,
  minorToInputText,
  parseEuropeanDateToISO,
  type Currency,
} from './money'
import { applyLocale, resetI18nForTests, type Locale } from './i18n'
import { formatPercent } from './arc'

const EUR: Currency = { code: 'EUR', decimals: 2 }
const JPY: Currency = { code: 'JPY', decimals: 0 }
const KWD: Currency = { code: 'KWD', decimals: 3 }

describe('parseMajorToMinor', () => {
  test('dot and comma decimals', () => {
    expect(parseMajorToMinor('25.50', EUR)).toBe(2550)
    expect(parseMajorToMinor('25,50', EUR)).toBe(2550)
    expect(parseMajorToMinor('25', EUR)).toBe(2500)
    expect(parseMajorToMinor(' 25,5 ', EUR)).toBe(2550)
  })

  test('thousand separators', () => {
    expect(parseMajorToMinor('1.234,56', EUR)).toBe(123456)
    expect(parseMajorToMinor('1,234.56', EUR)).toBe(123456)
    expect(parseMajorToMinor('1.234.567', EUR)).toBe(123456700)
  })

  test('currency symbols and negatives', () => {
    expect(parseMajorToMinor('€ 25.50', EUR)).toBe(2550)
    expect(parseMajorToMinor('-25.50', EUR)).toBe(-2550)
  })

  test('rejects garbage and over-precise fractions', () => {
    expect(parseMajorToMinor('', EUR)).toBeNull()
    expect(parseMajorToMinor('abc', EUR)).toBeNull()
    expect(parseMajorToMinor('25.5055', EUR)).toBeNull()
  })

  test('zero-decimal currency', () => {
    expect(parseMajorToMinor('1234', JPY)).toBe(1234)
    expect(parseMajorToMinor('12.34', JPY)).toBeNull()
  })

  test('three-decimal currency', () => {
    expect(parseMajorToMinor('12.345', KWD)).toBe(12345)
    expect(parseMajorToMinor('12', KWD)).toBe(12000)
  })
})

/**
 * What this runtime's own currency data says, read here only to prove the
 * tests below inject a number that differs from it. Production code must not
 * do this; `moneyDecimals.test.ts` fails if it does.
 */
function runtimeDecimals(code: string): number {
  const options = new Intl.NumberFormat('en', { style: 'currency', currency: code }).resolvedOptions()

  return options.maximumFractionDigits ?? Number.NaN
}

describe('the number of decimals is the one core sent', () => {
  // HUF is in the currency picker and is a code CLDR changed: 2 decimals up
  // to version 46, none from version 48. Whichever this runtime has, one of
  // the two HUF cases disagrees with it, and JPY with 2 always does.
  const cases: Array<{ currency: Currency; minor: number; shown: string; typed: string }> = [
    { currency: { code: 'HUF', decimals: 2 }, minor: 123456, shown: '1,234.56', typed: '1234.56' },
    { currency: { code: 'HUF', decimals: 0 }, minor: 123456, shown: '123,456', typed: '123456' },
    { currency: { code: 'JPY', decimals: 2 }, minor: 123456, shown: '1,234.56', typed: '1234.56' },
    { currency: { code: 'EUR', decimals: 3 }, minor: 123456, shown: '123.456', typed: '123.456' },
  ]

  test('the cases inject numbers this runtime would not have chosen', () => {
    const differing = cases
      .filter(({ currency }) => runtimeDecimals(currency.code) !== currency.decimals)
      .map(({ currency }) => currency.code)

    expect(differing).toContain('HUF')
    expect(differing).toContain('JPY')
    expect(differing).toContain('EUR')
  })

  test.each(cases)('formats $currency.code with $currency.decimals decimals', ({ currency, minor, shown }) => {
    const text = formatMoney(minor, currency, 'en-US')

    expect(text.replace(/[^\d.,]/g, '')).toBe(shown)
  })

  test.each(cases)('parses $currency.code with $currency.decimals decimals', ({ currency, minor, typed }) => {
    expect(parseMajorToMinor(typed, currency)).toBe(minor)
  })

  test.each(cases)(
    'writes $currency.code for an amount input with $currency.decimals decimals',
    ({ currency, minor, typed }) => {
      expect(minorToInputText(minor, currency)).toBe(typed)
      expect(parseMajorToMinor(minorToInputText(minor, currency), currency)).toBe(minor)
    },
  )

  test('a fraction longer than core allows is refused, whatever the runtime allows', () => {
    expect(parseMajorToMinor('12.5', { code: 'HUF', decimals: 0 })).toBeNull()
    expect(parseMajorToMinor('12.3456', { code: 'JPY', decimals: 2 })).toBeNull()
  })

  test('the sign is written once, beside the scaled number', () => {
    expect(formatMoney(-150, EUR, 'en-US')).toBe('-€1.50')
    expect(formatMoney(150, EUR, 'en-US', { signed: true })).toBe('+€1.50')
    expect(formatMoney(0, EUR, 'en-US', { signed: true })).toBe('€0.00')
  })
})

describe('amountProblemKey', () => {
  test('text that is not a number gets the screen\'s own wording', () => {
    expect(amountProblemKey(parseMajorToMinor('abc', EUR), 'tx.invalidAmount')).toBe(
      'tx.invalidAmount',
    )
  })

  test.each(['0', '0,00', '-5'])('%s is a number but not a postable amount', (text) => {
    expect(amountProblemKey(parseMajorToMinor(text, EUR), 'tx.invalidAmount')).toBe(
      'error.amountNotPositive',
    )
  })

  test('a positive amount has no problem', () => {
    expect(amountProblemKey(parseMajorToMinor('25,50', EUR), 'tx.invalidAmount')).toBeNull()
  })
})

describe('minorToInputText', () => {
  test('pads a small amount to a whole digit and the full fraction', () => {
    expect(minorToInputText(5, EUR)).toBe('0.05')
    expect(minorToInputText(0, EUR)).toBe('0.00')
    expect(minorToInputText(5, KWD)).toBe('0.005')
    expect(minorToInputText(5, JPY)).toBe('5')
  })

  test('keeps the sign and involves no rounding', () => {
    expect(minorToInputText(-2550, EUR)).toBe('-25.50')
    expect(minorToInputText(9007199254740991, EUR)).toBe('90071992547409.91')
  })
})

describe('bookCurrency', () => {
  test('takes the code and the decimals from the entity core sent', () => {
    expect(bookCurrency({ base_currency: 'KWD', base_currency_decimals: 3 })).toEqual(KWD)
    expect(bookCurrency({ base_currency: 'JPY', base_currency_decimals: 0 })).toEqual(JPY)
  })

  test.each([undefined, null, Number.NaN, 2.5, -1, 21, '2'])(
    'refuses %s for the decimals instead of guessing a scale',
    (decimals) => {
      const entity = { base_currency: 'EUR', base_currency_decimals: decimals as number }

      expect(() => bookCurrency(entity)).toThrow(/base_currency_decimals/)
    },
  )
})

describe('formatDate', () => {
  test('renders ISO strings as dd/mm/yyyy', () => {
    expect(formatDate('2026-03-15')).toBe('15/03/2026')
    expect(formatDate('2026-03-15T00:00:00')).toBe('15/03/2026')
  })

  test('renders {year, month, day} objects as dd/mm/yyyy', () => {
    expect(formatDate({ year: 2026, month: 'March', day: 5 })).toBe('05/03/2026')
    expect(formatDate({ year: 2026, month: 3, day: 5 })).toBe('05/03/2026')
  })

  test('leaves non-date strings untouched', () => {
    expect(formatDate('yesterday')).toBe('yesterday')
  })
})

describe('isoDate', () => {
  test('normalizes both entry date shapes to ISO', () => {
    expect(isoDate('2026-03-15T00:00:00')).toBe('2026-03-15')
    expect(isoDate({ year: 2026, month: 'March', day: 5 })).toBe('2026-03-05')
  })
})

describe('parseEuropeanDateToISO', () => {
  test('accepts day-first forms with mixed separators', () => {
    expect(parseEuropeanDateToISO('15/03/2026')).toBe('2026-03-15')
    expect(parseEuropeanDateToISO('5.3.26')).toBe('2026-03-05')
    expect(parseEuropeanDateToISO('01-12-2026')).toBe('2026-12-01')
    expect(parseEuropeanDateToISO('29/02/2024')).toBe('2024-02-29')
  })

  test('rejects impossible dates and garbage', () => {
    expect(parseEuropeanDateToISO('29/02/2026')).toBeNull()
    expect(parseEuropeanDateToISO('31/04/2026')).toBeNull()
    expect(parseEuropeanDateToISO('00/01/2026')).toBeNull()
    expect(parseEuropeanDateToISO('nope')).toBeNull()
  })

  test('rejects years outside 1900 to 2100', () => {
    expect(parseEuropeanDateToISO('01/01/1890')).toBeNull()
    expect(parseEuropeanDateToISO('01/01/0999')).toBeNull()
    expect(parseEuropeanDateToISO('2101-01-01')).toBeNull()
    expect(parseEuropeanDateToISO('01/01/1900')).toBe('1900-01-01')
    expect(parseEuropeanDateToISO('31/12/2100')).toBe('2100-12-31')
  })

  test('accepts ISO as a fallback', () => {
    expect(parseEuropeanDateToISO('2026-03-15')).toBe('2026-03-15')
  })
})

describe('one format for the app language', () => {
  const HUF: Currency = { code: 'HUF', decimals: 2 }
  const USD: Currency = { code: 'USD', decimals: 2 }
  const norm = (text: string) => text.replace(/[\s  ]/gu, ' ')

  afterEach(() => resetI18nForTests())

  test.each<[Locale, string]>([
    ['en', '€2,450.00'],
    ['el', '2.450,00 €'],
    ['fr', '2 450,00 €'],
    ['de', '2.450,00 €'],
  ])('%s writes a euro amount as %s, whatever the system locale is', (locale, expected) => {
    applyLocale(locale)

    expect(norm(formatMoney(245000, EUR))).toBe(expected)
  })

  test('HUF and JPY are grouped like every other currency', () => {
    applyLocale('en')

    expect(formatMoney(42983050, HUF)).toContain('429,830.50')
    expect(formatMoney(-1280, JPY)).toBe('-¥1,280')
    applyLocale('el')
    expect(norm(formatMoney(1234567, JPY))).toBe('1.234.567 JP¥')
  })

  test('USD is not forced to US formatting in another language', () => {
    applyLocale('de')

    expect(norm(formatMoney(123456, USD))).toBe('1.234,56 $')
  })

  test('the edit form is prefilled with the same decimal mark the list shows', () => {
    applyLocale('el')

    expect(minorToInputText(2550, EUR)).toBe('25,50')
    expect(parseMajorToMinor(minorToInputText(2550, EUR), EUR)).toBe(2550)
    expect(minorToInputText(1280, JPY)).toBe('1280')
  })

  test('percentages use the language of the money beside them', () => {
    applyLocale('en')
    expect(formatPercent(8210)).toBe('82.1%')
    expect(formatPercent(184480)).toBe('1,844.8%')
    applyLocale('fr')
    expect(norm(formatPercent(8210))).toBe('82,1 %')
  })
})
