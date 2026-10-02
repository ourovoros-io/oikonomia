import { afterEach, describe, expect, test, vi } from 'vitest'
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
    expect(placeholders(t(NOTE_CODE_KEYS.dated_from_document))).toEqual(['date'])
    expect(placeholders(t(NOTE_CODE_KEYS.amount_assumes_two_decimals))).toEqual(['currency'])
  })
})

describe('renderUiText', () => {
  const fee = { code: 'transfer_fee', params: { fee_minor: '140', currency: 'EUR' } }

  test.each([
    ['en', 'en-US', 'Transfer fee '],
    ['el', 'el-GR', 'Η προμήθεια εμβάσματος '],
    ['fr', 'fr-FR', 'Les frais de virement de '],
    ['de', 'de-DE', 'Die Überweisungsgebühr '],
  ] as const)('%s formats the fee as money in that language', (locale, tag, start) => {
    setLocale(locale)

    const text = renderUiText(fee)

    expect(text.startsWith(start)).toBe(true)
    expect(text).toContain(formatMoney(140, 'EUR', tag))
    expect(text).not.toContain('{')
  })

  test('the money follows the currency Rust names, not a number it formatted', () => {
    setLocale('en')

    const text = renderUiText({ code: 'transfer_fee', params: { fee_minor: '12345', currency: 'USD' } })

    expect(text).toContain('$123.45')
  })

  test('a date is shown the way the rest of the app shows dates', () => {
    setLocale('en')

    expect(renderUiText({ code: 'dated_from_document', params: { date: '2026-03-15' } })).toContain(
      '15/03/2026',
    )
  })

  test('a plain value is filled in as it is', () => {
    setLocale('de')

    expect(
      renderUiText({ code: 'amount_assumes_two_decimals', params: { currency: 'JPY' } }),
    ).toContain('JPY')
  })

  test('an unknown code renders nothing and warns', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(renderUiText({ code: 'a_code_from_a_newer_version' })).toBe('')
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('a_code_from_a_newer_version'))
  })

  test('a name inherited from Object.prototype is not a code', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(renderUiText({ code: 'constructor' })).toBe('')
    expect(renderUiText({ code: 'toString' })).toBe('')
  })

  test('a fee without its currency renders nothing and warns', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(renderUiText({ code: 'transfer_fee', params: { fee_minor: '140' } })).toBe('')
    expect(warn).toHaveBeenCalled()
  })

  test('notes are joined in order and unknown ones are skipped', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    setLocale('en')

    expect(
      renderUiTexts([{ code: 'ocr_read' }, { code: 'nope' }, { code: 'invoice_parsed' }]),
    ).toBe(
      'Read with built-in offline OCR. Review before saving. Parsed offline with the built-in invoice reader (no internet).',
    )
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
