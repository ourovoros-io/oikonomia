import { afterEach, describe, expect, test, vi } from 'vitest'
import { FILE_TEXT_LIMIT } from './fileText'
import { resetI18nForTests, resolvesInLocale, setLocale, t, type Locale } from './i18n'
import { formatMoney } from './money'
import {
  HINT_KEYS,
  NOTE_CODE_KEYS,
  SYNTHETIC_LINE_KEYS,
  renderAnalyzerHint,
  renderUiText,
  renderUiTexts,
  reportLineName,
} from './uiText'
import codes from './uiTextCodes.json'

const LOCALES: readonly Locale[] = ['en', 'el', 'fr', 'de']

afterEach(() => {
  resetI18nForTests()
  vi.restoreAllMocks()
})

function placeholders(text: string): string[] {
  return [...new Set([...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]))].sort()
}

describe('code lists pinned to Rust', () => {
  // uiTextCodes.json is checked against the Rust enums by a Rust test, so
  // together these keep both sides in step.
  test('every note code Rust can emit has a catalog key, and no other', () => {
    expect(Object.keys(NOTE_CODE_KEYS).sort()).toEqual([...codes.notes].sort())
  })

  test('every hint code Rust can emit has a catalog key, and no other', () => {
    expect(Object.keys(HINT_KEYS).sort()).toEqual([...codes.hints].sort())
  })

  test('every synthetic row kind Rust can emit has a catalog key, and no other', () => {
    expect(Object.keys(SYNTHETIC_LINE_KEYS).sort()).toEqual([...codes.syntheticLines].sort())
  })
})

describe('copy for every code', () => {
  const keys = [
    ...Object.values(NOTE_CODE_KEYS),
    ...Object.values(HINT_KEYS),
    ...Object.values(SYNTHETIC_LINE_KEYS),
  ]

  test.each(LOCALES)('%s has copy for every key', (locale) => {
    const missing = keys.filter((key) => !resolvesInLocale(locale, key))

    expect(missing).toEqual([])
  })

  test.each(LOCALES)('%s uses the same placeholders as English', (locale) => {
    const mismatched: string[] = []

    for (const key of keys) {
      setLocale('en')
      const english = placeholders(t(key))

      setLocale(locale)
      const other = placeholders(t(key))

      if (other.join() !== english.join()) mismatched.push(`${key}: {${english}} vs {${other}}`)
    }

    expect(mismatched).toEqual([])
  })

  test('a note that carries values has the matching placeholder', () => {
    setLocale('en')

    expect(placeholders(t(NOTE_CODE_KEYS.transfer_fee))).toEqual(['fee'])
    expect(placeholders(t(NOTE_CODE_KEYS.transfer_fee_unstated))).toEqual([])
    expect(placeholders(t(NOTE_CODE_KEYS.dated_from_document))).toEqual(['date'])
    expect(placeholders(t(NOTE_CODE_KEYS.amount_assumes_two_decimals))).toEqual(['currency'])
    expect(placeholders(t(NOTE_CODE_KEYS.csv_invalid_date))).toEqual(['value'])
    expect(placeholders(t(NOTE_CODE_KEYS.csv_invalid_amount))).toEqual(['value'])
    expect(placeholders(t(NOTE_CODE_KEYS.csv_invalid_type))).toEqual(['value'])
    expect(placeholders(t(NOTE_CODE_KEYS.csv_missing_date))).toEqual([])
    expect(placeholders(t(NOTE_CODE_KEYS.csv_missing_amount))).toEqual([])
    expect(placeholders(t(NOTE_CODE_KEYS.csv_zero_amount))).toEqual([])
    expect(placeholders(t(NOTE_CODE_KEYS.csv_amount_overflow))).toEqual([])
    expect(placeholders(t(NOTE_CODE_KEYS.csv_unreadable_row))).toEqual([])
  })
})

const EUR = { code: 'EUR', decimals: 2 }

describe('renderUiText', () => {
  const fee = { code: 'transfer_fee', params: { fee_minor: '140', currency: 'EUR' } }

  test.each([
    ['en', 'Transfer fee '],
    ['el', 'Η προμήθεια εμβάσματος '],
    ['fr', 'Les frais de virement de '],
    ['de', 'Die Überweisungsgebühr von '],
  ] as const)('%s words the fee in that language', (locale, start) => {
    setLocale(locale)

    const text = renderUiText(fee, EUR)

    expect(text.startsWith(start)).toBe(true)
    expect(text).not.toContain('{')
  })

  test.each(LOCALES)(
    '%s shows the fee the way the amount beside it is shown, whatever the language',
    (locale) => {
      setLocale(locale)

      // The suggested amount in the same banner is formatMoney(minor, currency).
      expect(renderUiText(fee, EUR)).toContain(formatMoney(140, EUR))
    },
  )

  test('a EUR fee reads the same number in every language', () => {
    const shown = LOCALES.map((locale) => {
      setLocale(locale)
      return renderUiText(fee, EUR).match(/1,40\s€/u)?.[0]
    })

    expect(shown).toEqual(Array(LOCALES.length).fill(formatMoney(140, EUR)))
  })

  test.each([
    ['en', 'csv_missing_date', 'The date is missing.'],
    ['el', 'csv_missing_date', 'Λείπει η ημερομηνία.'],
    ['fr', 'csv_missing_date', 'La date est manquante.'],
    ['de', 'csv_missing_date', 'Das Datum fehlt.'],
    ['en', 'csv_missing_amount', 'The amount is missing.'],
    ['el', 'csv_missing_amount', 'Λείπει το ποσό.'],
    ['fr', 'csv_missing_amount', 'Le montant est manquant.'],
    ['de', 'csv_missing_amount', 'Der Betrag fehlt.'],
  ] as const)('%s words %s as a sentence with no empty quotation marks', (locale, code, copy) => {
    setLocale(locale)

    expect(renderUiText({ code }, EUR)).toBe(copy)
  })

  test('the fee that is not a figure has plain copy in every language', () => {
    for (const locale of LOCALES) {
      setLocale(locale)

      const text = renderUiText({ code: 'transfer_fee_unstated' }, EUR)

      expect(text).not.toBe('')
      expect(text).not.toContain('{')
    }
  })

  test.each([
    ['en', 'A transfer fee is shown on the receipt and is not the posted amount.'],
    [
      'el',
      'Στην απόδειξη αναγράφεται προμήθεια εμβάσματος, η οποία δεν είναι το ποσό που καταχωρίζεται.',
    ],
    ['fr', 'Des frais de virement figurent sur le reçu et ne sont pas le montant enregistré.'],
    ['de', 'Auf dem Beleg steht eine Überweisungsgebühr; sie ist nicht der gebuchte Betrag.'],
  ] as const)('%s copy for the fee without a figure', (locale, copy) => {
    setLocale(locale)

    expect(renderUiText({ code: 'transfer_fee_unstated' }, EUR)).toBe(copy)
  })

  test("the money is formatted in the book's currency, from the minor units Rust sent", () => {
    setLocale('en')

    const text = renderUiText(
      { code: 'transfer_fee', params: { fee_minor: '12345', currency: 'USD' } },
      { code: 'USD', decimals: 2 },
    )

    expect(text).toContain('$123.45')
  })

  test("the fee is scaled by the decimals core sent, not by the webview's for the currency", () => {
    setLocale('en')

    const text = renderUiText(
      { code: 'transfer_fee', params: { fee_minor: '12345', currency: 'JPY' } },
      { code: 'JPY', decimals: 2 },
    )

    expect(text).toContain('123.45')
  })

  test.each([
    ['no book', null],
    ['a book in another currency', { code: 'USD', decimals: 2 }],
  ])('a fee with %s renders nothing and warns', (_label, book) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(renderUiText(fee, book)).toBe('')
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('transfer_fee'))
  })

  test('a long cell of the file is cut, and markup in it stays text', () => {
    setLocale('en')
    const long = 'x'.repeat(FILE_TEXT_LIMIT * 3)

    expect(renderUiText({ code: 'csv_invalid_amount', params: { value: long } }, EUR)).toBe(
      `"${'x'.repeat(FILE_TEXT_LIMIT)}…" is not a valid amount.`,
    )
    expect(renderUiText({ code: 'csv_invalid_type', params: { value: '<i>in</i>' } }, EUR)).toBe(
      '"<i>in</i>" is not a recognized type.',
    )
  })

  test('a date is shown the way the rest of the app shows dates', () => {
    setLocale('en')

    expect(renderUiText({ code: 'dated_from_document', params: { date: '2026-03-15' } }, EUR)).toContain(
      '15/03/2026',
    )
  })

  test('a plain value is filled in as it is', () => {
    setLocale('de')

    expect(
      renderUiText({ code: 'amount_assumes_two_decimals', params: { currency: 'JPY' } }, EUR),
    ).toContain('JPY')
  })

  test('an unknown code renders nothing and warns', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(renderUiText({ code: 'a_code_from_a_newer_version' }, EUR)).toBe('')
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('a_code_from_a_newer_version'))
  })

  test('a name inherited from Object.prototype is not a code', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(renderUiText({ code: 'constructor' }, EUR)).toBe('')
    expect(renderUiText({ code: 'toString' }, EUR)).toBe('')
  })

  test('a fee without its currency renders nothing and warns', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(renderUiText({ code: 'transfer_fee', params: { fee_minor: '140' } }, EUR)).toBe('')
    expect(warn).toHaveBeenCalled()
  })

  test.each([
    ['empty', ''],
    ['a decimal', '1.5'],
    ['not a number', 'abc'],
    ['padded', ' 140'],
  ])('a fee that is %s renders nothing and warns, never zero', (_label, minor) => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(
      renderUiText({ code: 'transfer_fee', params: { fee_minor: minor, currency: 'EUR' } }, EUR),
    ).toBe('')
    expect(warn).toHaveBeenCalled()
  })

  test('a negative whole number is still a number', () => {
    setLocale('en')

    expect(
      renderUiText({ code: 'transfer_fee', params: { fee_minor: '-140', currency: 'EUR' } }, EUR),
    ).toContain(formatMoney(-140, EUR))
  })

  test('a note whose value is missing renders nothing and warns, for any value', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    setLocale('en')

    expect(renderUiText({ code: 'dated_from_document' }, EUR)).toBe('')
    expect(renderUiText({ code: 'dated_from_document', params: {} }, EUR)).toBe('')
    expect(renderUiText({ code: 'amount_assumes_two_decimals' }, EUR)).toBe('')
    expect(warn).toHaveBeenCalledTimes(3)
  })

  test('notes are joined in order and unknown ones are skipped', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    setLocale('en')

    expect(
      renderUiTexts([{ code: 'ocr_read' }, { code: 'nope' }, { code: 'invoice_parsed' }], EUR),
    ).toBe(
      'Read with built-in offline OCR. Review before saving. Parsed offline with the built-in invoice reader (no internet).',
    )
  })
})

describe('copy polish', () => {
  test.each([
    ['el', 'ocr_failed', 'Η OCR απέτυχε. Μπορείτε ωστόσο να συμπληρώσετε τα πεδία.'],
    ['fr', 'ocr_failed', 'L’OCR a échoué. Vous pouvez tout de même remplir les champs.'],
    [
      'de',
      'ocr_failed',
      'Die OCR ist fehlgeschlagen. Sie können die Felder trotzdem selbst ausfüllen.',
    ],
    ['de', 'invoice_unpaid', 'Als Kauf auf Rechnung / offener Betrag vermerkt.'],
    ['el', 'invoice_unpaid', 'Σημειώθηκε ως αγορά επί πιστώσει / οφειλόμενο ποσό.'],
    ['fr', 'invoice_unpaid', 'Marqué comme achat à crédit / montant dû.'],
  ] as const)('%s %s', (locale, code, copy) => {
    setLocale(locale)

    expect(renderUiText({ code }, EUR)).toBe(copy)
  })

  test.each([
    [
      'de',
      'Die Überweisungsgebühr von 1,40\u00a0€ steht auf dem Beleg und ist nicht der gebuchte Betrag.',
    ],
    [
      'el',
      'Η προμήθεια εμβάσματος 1,40\u00a0€ αναγράφεται στην απόδειξη και δεν είναι το ποσό που καταχωρίζεται.',
    ],
  ] as const)('%s fee note', (locale, copy) => {
    setLocale(locale)

    expect(
      renderUiText({ code: 'transfer_fee', params: { fee_minor: '140', currency: 'EUR' } }, EUR),
    ).toBe(copy)
  })

  test.each([
    [
      'fr',
      'La détection du montant suppose des devises à 2 décimales\u202f; saisissez le montant en JPY à la main.',
    ],
    [
      'de',
      'Die Betragserkennung geht von Währungen mit 2 Dezimalstellen aus; tragen Sie den Betrag in JPY selbst ein.',
    ],
  ] as const)('%s amount note', (locale, copy) => {
    setLocale(locale)

    expect(
      renderUiText({ code: 'amount_assumes_two_decimals', params: { currency: 'JPY' } }, EUR),
    ).toBe(copy)
  })
})

describe('hints and synthetic rows', () => {
  test('a hint is worded in the current language', () => {
    setLocale('fr')

    expect(renderAnalyzerHint('ready')).toBe(
      'Lecteur de factures hors ligne intégré + OCR — rien ne quitte cet appareil.',
    )
  })

  test('an unknown hint renders nothing and warns', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(renderAnalyzerHint('mystery')).toBe('')
    expect(warn).toHaveBeenCalled()
  })

  test('a computed report row shows its label, a real one its own name', () => {
    setLocale('de')

    expect(reportLineName({ name: 'Net Income (current period)', synthetic: 'net_income' })).toBe(
      'Periodenergebnis (aktuelle Periode)',
    )
    expect(reportLineName({ name: 'Checking', synthetic: null })).toBe('Checking')
    expect(reportLineName({ name: 'Checking' })).toBe('Checking')
  })
})
