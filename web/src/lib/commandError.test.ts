import { describe, expect, it } from 'vitest'
import codes from './errorCodes.json'
import { commandErrorMessage, ERROR_CODE_KEYS } from './commandError'
import { flattenMessages } from './i18n'
import en from '../locales/en.json' with { type: 'json' }
import el from '../locales/el.json' with { type: 'json' }
import fr from '../locales/fr.json' with { type: 'json' }
import de from '../locales/de.json' with { type: 'json' }

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

  it('falls back to the raw message for unknown codes', () => {
    expect(commandErrorMessage({ code: 'brand_new', message: 'raw text' })).toBe('raw text')
  })
})
