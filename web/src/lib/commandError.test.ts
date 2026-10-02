import { afterEach, describe, expect, it, vi } from 'vitest'
import codes from './errorCodes.json'
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
      account_wrong_type: ['code'],
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
})
