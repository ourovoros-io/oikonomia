import { afterEach, describe, expect, it, vi } from 'vitest'
import codes from './errorCodes.json'
import roles from './accountRoles.json'
import { asCommandError, commandErrorMessage, ERROR_CODE_KEYS, fileReadError } from './commandError'
import { flattenMessages, LOCALES, resetI18nForTests, setLocale } from './i18n'
import en from '../locales/en.json' with { type: 'json' }
import el from '../locales/el.json' with { type: 'json' }
import fr from '../locales/fr.json' with { type: 'json' }
import de from '../locales/de.json' with { type: 'json' }

afterEach(() => {
  vi.restoreAllMocks()
  resetI18nForTests()
})

describe('command error localization', () => {
  it('maps every canonical code in every locale', () => {
    const catalogs = { en, el, fr, de }
    for (const code of codes) {
      const key = ERROR_CODE_KEYS[code]
      expect(key, `no i18n key for code ${code}`).toBeTruthy()
      for (const [locale, catalog] of Object.entries(catalogs)) {
        const flat = flattenMessages(catalog)
        expect(flat[key], `${locale} missing ${key}`).toBeTruthy()
      }
    }
  })

  it('has copy that uses exactly the parameters Rust sends, in every locale', () => {
    // Mirrors ValidationError::params in oikonomia-core; the Rust tests pin the other side.
    const codeParams: Record<string, string[]> = {
      password_too_short: ['min'],
      name_taken: ['name'],
      account_inactive: ['code'],
      account_required: ['role'],
      account_wrong_type: ['code', 'role'],
      invalid_date: ['value'],
      lock_timeout_too_short: ['min_secs'],
      file_too_large: ['max_mb'],
    }
    const catalogs = { en, el, fr, de }

    for (const code of codes) {
      const expected = [...(codeParams[code] ?? [])].sort()
      for (const [locale, catalog] of Object.entries(catalogs)) {
        const copy = flattenMessages(catalog)[ERROR_CODE_KEYS[code]]
        const used = [...new Set([...copy.matchAll(/\{(\w+)\}/g)].map((m) => m[1]))].sort()
        expect(used, `${locale} copy for ${code}`).toEqual(expected)
      }
    }
    for (const code of Object.keys(codeParams)) {
      expect(codes, `${code} is not in errorCodes.json`).toContain(code)
    }
  })

  it('fills the copy in from the error params', () => {
    expect(
      commandErrorMessage({
        code: 'password_too_short',
        message: 'password must be at least 12 characters',
        params: { min: '12' },
      }),
    ).toBe('That password is too short. Use at least 12 characters.')
  })

  it('falls back to the screen sentence for an unknown code, never the raw message', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    const shown = commandErrorMessage(
      { code: 'brand_new', message: 'sqlcipher: disk image is malformed' },
      'settings.deleteFailed',
    )

    expect(shown).toBe('Could not delete the entity.')
    expect(shown).not.toContain('sqlcipher')
    // The raw text is kept for diagnosis.
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('sqlcipher: disk image is malformed'))
  })

  it('uses the generic copy when an unknown code comes with no screen fallback', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(commandErrorMessage({ code: 'brand_new', message: 'raw text' })).toBe('Something went wrong.')
  })

  it('prefers the screen sentence over the generic copy for the catch-all unknown code', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(commandErrorMessage({ code: 'unknown', message: 'raw text' }, 'app.failedLock')).toBe(
      'Could not lock the vault.',
    )
  })

  it('shows the copy for a known code and ignores the screen fallback', () => {
    expect(
      commandErrorMessage(
        { code: 'name_taken', message: 'raw', params: { name: 'Personal' } },
        'settings.deleteFailed',
      ),
    ).toBe('The name \u201cPersonal\u201d is already in use. Choose a different name.')
  })

  it('localizes by code in every language', () => {
    const error = { code: 'amount_not_positive', message: 'amount must be positive' }
    const shown = new Set<string>()

    for (const locale of LOCALES) {
      setLocale(locale)
      shown.add(commandErrorMessage(error))
    }

    expect(shown.size).toBe(LOCALES.length)
    expect(shown).not.toContain('amount must be positive')
  })

  it('accepts anything invoke can throw and keeps code and params', () => {
    expect(asCommandError('boom')).toEqual({ code: 'unknown', message: 'boom' })
    expect(asCommandError(new Error('boom'))).toEqual({ code: 'unknown', message: 'boom' })
    expect(asCommandError({ error: 'boom' })).toEqual({ code: 'unknown', message: 'boom' })
    expect(
      asCommandError({ code: 'file_too_large', message: 'm', params: { max_mb: '8' } }),
    ).toEqual({ code: 'file_too_large', message: 'm', params: { max_mb: '8' } })
  })

  it('localizes an error raised by the web layer itself', () => {
    expect(commandErrorMessage(fileReadError())).toBe('Could not read the file')

    setLocale('el')
    expect(commandErrorMessage(fileReadError())).toBe('Δεν ήταν δυνατή η ανάγνωση του αρχείου')
  })

  it('names the missing account in every language, with the form\u2019s own label', () => {
    const error = {
      code: 'account_required',
      message: 'category account is required',
      params: { role: 'category' },
    }
    const expected = {
      en: 'Category (what for)',
      el: 'Κατηγορία (για τι)',
      fr: 'Catégorie (pour quoi)',
      de: 'Kategorie (wofür)',
    }

    for (const locale of LOCALES) {
      setLocale(locale)
      const shown = commandErrorMessage(error)

      expect(shown, locale).toContain(expected[locale])
      expect(shown, locale).not.toContain('{')
      expect(shown, locale).not.toContain('category account')
    }
  })

  it('names the role and the account for a wrong account type in every language', () => {
    const error = {
      code: 'account_wrong_type',
      message: 'payment account 5100 has the wrong type for this entry',
      params: { role: 'payment', code: '5100' },
    }
    const expected = { en: 'Paid from', el: 'Πληρωμή από', fr: 'Payé depuis', de: 'Bezahlt von' }

    for (const locale of LOCALES) {
      setLocale(locale)
      const shown = commandErrorMessage(error)

      expect(shown, locale).toContain(expected[locale])
      expect(shown, locale).toContain('5100')
    }
  })

  it('has a label for every role in every language', () => {
    for (const role of roles) {
      for (const locale of LOCALES) {
        setLocale(locale)
        const shown = commandErrorMessage({
          code: 'account_required',
          message: 'm',
          params: { role },
        })

        expect(shown, `${locale} ${role}`).not.toContain(role)
        expect(shown, `${locale} ${role}`).not.toContain('{')
      }
    }
  })

  it('falls back to the sentence without a role when the role has no label', () => {
    for (const locale of LOCALES) {
      setLocale(locale)

      for (const params of [{ role: 'mystery' }, undefined]) {
        const shown = commandErrorMessage({ code: 'account_required', message: 'm', params })

        expect(shown, locale).not.toContain('mystery')
        expect(shown, locale).not.toContain('{')
        expect(shown, locale).not.toContain('error.')
      }
    }

    setLocale('en')
    expect(
      commandErrorMessage({
        code: 'account_wrong_type',
        message: 'm',
        params: { role: 'mystery', code: '5100' },
      }),
    ).toBe('Account 5100 is the wrong type for this entry. Choose a different account.')
  })
})
