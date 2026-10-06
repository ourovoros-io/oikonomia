import { afterEach, describe, expect, it, vi } from 'vitest'
import codes from './errorCodes.json'
import roles from './accountRoles.json'
import codeParams from './errorCodeParams.json'
import {
  asCommandError,
  commandErrorMessage,
  ERROR_CODE_KEYS,
  fileReadError,
  isMissingIpcCommand,
  logCommandError,
  WEB_ERROR_KEYS,
} from './commandError'
import { flattenMessages, LOCALES, resetI18nForTests, setLocale, t } from './i18n'
import en from '../locales/en.json' with { type: 'json' }
import el from '../locales/el.json' with { type: 'json' }
import fr from '../locales/fr.json' with { type: 'json' }
import de from '../locales/de.json' with { type: 'json' }

afterEach(() => {
  vi.restoreAllMocks()
  resetI18nForTests()
})

/**
 * Parameters Rust sends that no copy uses yet, by code. Each is a value the
 * wording could name (which field is blank, what was not found) once someone
 * writes that wording in every language. Until then the copy must not use
 * them, and an entry here that Rust stops sending fails the test below.
 */
const UNWORDED_PARAMS: Record<string, string[]> = {
  csv_invalid_amount: ['value'],
  csv_invalid_date: ['value'],
  csv_invalid_integer: ['value'],
  csv_invalid_mapping: ['column', 'problem'],
  csv_invalid_status: ['value'],
  csv_invalid_type: ['value'],
  csv_missing_column: ['column'],
  name_required: ['field'],
}

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

  it('maps no code that errorCodes.json does not list', () => {
    // The map holds only codes Rust can send. The codes the web layer raises
    // itself (file_read_failed) live in WEB_ERROR_KEYS and are not in the pin.
    expect(Object.keys(ERROR_CODE_KEYS).sort()).toEqual([...codes].sort())
    expect(Object.keys(WEB_ERROR_KEYS)).toEqual(['file_read_failed'])
  })

  it('has copy that uses exactly the parameters Rust sends, in every locale', () => {
    // errorCodeParams.json is pinned to the params() of the Rust error types by
    // Rust tests, so this is checked against what Rust sends, not a hand copy.
    // Where a parameter depends on the value, the file lists every name the
    // copy may use.
    const paramsByCode: Record<string, string[]> = codeParams
    const catalogs = { en, el, fr, de }

    for (const [code, unworded] of Object.entries(UNWORDED_PARAMS)) {
      for (const name of unworded) {
        expect(paramsByCode[code] ?? [], `${code} no longer sends ${name}`).toContain(name)
      }
    }

    for (const code of codes) {
      const unworded = UNWORDED_PARAMS[code] ?? []
      const expected = [...(paramsByCode[code] ?? [])].filter((name) => !unworded.includes(name)).sort()
      for (const [locale, catalog] of Object.entries(catalogs)) {
        const copy = flattenMessages(catalog)[ERROR_CODE_KEYS[code]]
        const used = [...new Set([...copy.matchAll(/\{(\w+)\}/g)].map((m) => m[1]))].sort()
        expect(used, `${locale} copy for ${code}`).toEqual(expected)
      }
    }
    for (const code of Object.keys(paramsByCode)) {
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

  it('never shows a raw placeholder when a parameter is missing', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    for (const locale of LOCALES) {
      setLocale(locale)

      const bare = commandErrorMessage({ code: 'name_taken', message: 'raw' })
      const withFallback = commandErrorMessage(
        { code: 'name_taken', message: 'raw', params: {} },
        'settings.deleteFailed',
      )

      expect(bare, locale).not.toMatch(/\{\w+\}/)
      expect(withFallback, locale).not.toMatch(/\{\w+\}/)
      expect(bare, locale).toBe(t('error.unknown'))
      expect(withFallback, locale).toBe(t('settings.deleteFailed'))
    }

    // The error is logged, so the missing value is not lost.
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('name_taken'))
  })

  it('falls back for a parameterised code whose params lack the needed name', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(
      commandErrorMessage({ code: 'password_too_short', message: 'raw', params: { other: '1' } }),
    ).toBe('Something went wrong.')
  })

  it('still shows a value that itself looks like a placeholder', () => {
    expect(
      commandErrorMessage({ code: 'name_taken', message: 'raw', params: { name: '{a}' } }),
    ).toContain('{a}')
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
    expect(commandErrorMessage(fileReadError())).toBe('Could not read the file.')

    setLocale('el')
    expect(commandErrorMessage(fileReadError())).toBe('Δεν ήταν δυνατή η ανάγνωση του αρχείου.')
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

  it('treats an account-type error without its account code as no copy', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    for (const locale of LOCALES) {
      setLocale(locale)

      for (const params of [undefined, { role: 'payment' }, { role: 'mystery' }]) {
        const error = { code: 'account_wrong_type', message: 'm', params }
        const unknown = commandErrorMessage(error)

        expect(unknown, locale).not.toContain('{')
        expect(unknown, locale).toBe(t('error.unknown'))
        expect(commandErrorMessage(error, 'files.error.read'), locale).toBe(t('files.error.read'))
      }
    }
  })

  // The codes whose copy says little; a screen's own sentence says more.
  const VAGUE_CODES = ['io', 'crypto', 'unknown', 'task_failed', 'validation_internal', 'analysis']

  it('lets the screen sentence replace the copy of every vague code', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    for (const code of VAGUE_CODES) {
      expect(commandErrorMessage({ code, message: 'raw' }, 'docs.deleteFailed'), code).toBe(
        'Could not delete the document.',
      )
    }
  })

  it('keeps the copy of a vague code when the caller gave no screen sentence', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    for (const code of VAGUE_CODES) {
      const shown = commandErrorMessage({ code, message: 'raw' })

      expect(shown, code).toBe(t(ERROR_CODE_KEYS[code]))
      expect(shown, code).not.toBe('Could not delete the document.')
    }

    expect(commandErrorMessage({ code: 'io', message: 'raw' })).toBe('A file operation failed.')
  })

  it('lets a specific code win over the screen sentence', () => {
    expect(
      commandErrorMessage({ code: 'name_taken', message: 'raw', params: { name: 'A' } }, 'docs.deleteFailed'),
    ).toContain('already in use')
  })

  it('logs the raw error for a vague code and for an unknown code', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    commandErrorMessage({ code: 'io', message: 'EACCES: /Users/x/vault' }, 'docs.deleteFailed')
    commandErrorMessage({ code: 'io', message: 'EACCES: /Users/x/vault' })
    expect(warn).toHaveBeenCalledTimes(2)
    expect(warn.mock.calls[0][0]).toContain('"io"')
    expect(warn.mock.calls[0][0]).toContain('EACCES: /Users/x/vault')

    warn.mockClear()
    commandErrorMessage({ code: 'brand_new', message: 'sqlcipher: bad page', params: { a: 'b' } })
    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn.mock.calls[0][0]).toContain('"brand_new"')
    expect(warn.mock.calls[0][0]).toContain('sqlcipher: bad page')
    expect(warn.mock.calls[0][0]).toContain('"a":"b"')
  })

  it('does not log a user-correctable error', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    commandErrorMessage({ code: 'name_taken', message: 'raw', params: { name: 'A' } }, 'docs.deleteFailed')
    commandErrorMessage({ code: 'invalid_password', message: 'raw' })

    expect(warn).not.toHaveBeenCalled()
  })

  it('logCommandError logs whatever was thrown, once, without throwing', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    logCommandError({ code: 'save_failed', message: 'ENOSPC' })
    logCommandError(undefined)

    expect(warn).toHaveBeenCalledTimes(2)
    expect(warn.mock.calls[0][0]).toContain('ENOSPC')
  })

  it('treats a code named like an Object.prototype member as unknown', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    for (const code of ['constructor', 'toString', '__proto__', 'hasOwnProperty']) {
      expect(commandErrorMessage({ code, message: 'raw' }, 'docs.deleteFailed'), code).toBe(
        'Could not delete the document.',
      )
      expect(commandErrorMessage({ code, message: 'raw' }), code).toBe('Something went wrong.')
    }
  })

  it('treats a role named like an Object.prototype member as having no label', () => {
    for (const role of ['constructor', '__proto__', 'toString']) {
      const shown = commandErrorMessage({ code: 'account_required', message: 'm', params: { role } })

      expect(shown, role).not.toContain(role)
      expect(shown, role).not.toContain('{')
    }
  })

  it('keeps the full stop on the file-read sentence in every language', () => {
    for (const locale of LOCALES) {
      setLocale(locale)

      expect(commandErrorMessage(fileReadError()), locale).toMatch(/\.$/)
    }
  })

  it('detects an unregistered Tauri command', () => {
    expect(isMissingIpcCommand({ code: 'unknown', message: 'command update_check not found' }, 'update_check')).toBe(
      true,
    )
    expect(isMissingIpcCommand({ code: 'io', message: 'disk full' }, 'update_check')).toBe(false)
  })
})
