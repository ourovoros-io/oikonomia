import { afterEach, describe, expect, it, vi } from 'vitest'
import codes from './errorCodes.json'
import roles from './accountRoles.json'
import codeParams from './errorCodeParams.json'
import mappingProblems from './csvMappingProblems.json'
import {
  asCommandError,
  commandErrorMessage,
  ERROR_CODE_KEYS,
  fileReadError,
  isMissingIpcCommand,
  logCommandError,
  VARIANT_KEYS,
  WEB_ERROR_KEYS,
} from './commandError'
import { FILE_TEXT_LIMIT } from './fileText'
import { flattenMessages, LOCALES, resetI18nForTests, setLocale, t } from './i18n'
import { NOTE_CODE_KEYS } from './uiText'
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
  analysis: ['operation'],
  crypto: ['operation'],
  database: ['operation'],
  io: ['operation'],
  name_required: ['field'],
  not_found: ['resource'],
  serialization: ['operation'],
  unbalanced_entry: ['credits', 'debits'],
  vault_too_new: ['found', 'supported'],
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

  it('resolves vault_unlock_before_backup to its own sentence, not the corrupt-vault one', () => {
    expect(ERROR_CODE_KEYS.vault_unlock_before_backup).toBe('error.vaultUnlockBeforeBackup')

    const sentences = {
      en: 'This vault needs to be unlocked once before it can be backed up. Unlock it, then try again; nothing has been lost.',
      el: 'Αυτή η θυρίδα πρέπει να ξεκλειδωθεί μία φορά πριν από το αντίγραφο ασφαλείας. Ξεκλειδώστε την και δοκιμάστε ξανά· δεν έχει χαθεί τίποτα.',
      fr: 'Ce coffre doit être déverrouillé une fois avant de pouvoir être sauvegardé. Déverrouillez-le, puis réessayez ; rien n’a été perdu.',
      de: 'Dieser Tresor muss einmal entsperrt werden, bevor er gesichert werden kann. Entsperren Sie ihn und versuchen Sie es dann erneut; es ist nichts verloren gegangen.',
    } satisfies Record<(typeof LOCALES)[number], string>
    const catalogs = { en, el, fr, de }

    for (const locale of LOCALES) {
      const flat = flattenMessages(catalogs[locale])
      const sentence = flat['error.vaultUnlockBeforeBackup']

      expect(sentence, locale).toBe(sentences[locale])
      expect(sentence, locale).not.toBe(flat['error.vaultCorrupt'])
    }

    const shown = commandErrorMessage({
      code: 'vault_unlock_before_backup',
      message: 'unmerged write-ahead log',
      params: {},
    })

    expect(shown).toBe(sentences.en)
  })

  it('resolves vault_too_new to its own key, not the corrupt-vault sentence', () => {
    expect(ERROR_CODE_KEYS.vault_too_new).toBe('error.vaultTooNew')
    expect(ERROR_CODE_KEYS.vault_too_new).not.toBe('error.vaultCorrupt')

    const writer = {
      en: 'This vault was saved by a newer version of Oikonomia. Update the app to open it; nothing has been lost.',
      el: 'Αυτή η θυρίδα αποθηκεύτηκε από νεότερη έκδοση του Oikonomia. Ενημερώστε την εφαρμογή για να την ανοίξετε· δεν έχει χαθεί τίποτα.',
      fr: 'Ce coffre a été enregistré par une version plus récente d’Oikonomia. Mettez à jour l’application pour l’ouvrir ; rien n’a été perdu.',
      de: 'Dieser Tresor wurde mit einer neueren Version von Oikonomia gespeichert. Aktualisieren Sie die App, um ihn zu öffnen; es ist nichts verloren gegangen.',
    }
    const catalogs = { en, el, fr, de }
    for (const [locale, catalog] of Object.entries(catalogs)) {
      const flat = flattenMessages(catalog)
      expect(flat['error.vaultTooNew'], locale).toBe(writer[locale as keyof typeof writer])
      expect(flat['error.vaultTooNew'], locale).not.toBe(flat['error.vaultCorrupt'])
    }

    const shown = commandErrorMessage({
      code: 'vault_too_new',
      message: 'vault schema version 8 is newer than this build supports (7)',
      params: { found: '8', supported: '7' },
    })

    expect(shown).toBe(writer.en)
    expect(shown).not.toBe(t('error.vaultCorrupt'))
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
      // The sentence of a code with variants is the one shown when the
      // identifier is unknown, so it can name no value; the test below
      // checks the variants against what Rust sends.
      const expected = Object.hasOwn(VARIANT_KEYS, code)
        ? []
        : [...(paramsByCode[code] ?? [])].filter((name) => !unworded.includes(name)).sort()
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

  it('has variant copy that together uses exactly the parameters Rust sends, in every locale', () => {
    const paramsByCode: Record<string, string[]> = codeParams
    const catalogs = { en, el, fr, de }

    for (const [code, { param, keys }] of Object.entries(VARIANT_KEYS)) {
      // The identifier chooses the sentence; it is never a value in one.
      const expected = (paramsByCode[code] ?? []).filter((name) => name !== param).sort()
      expect(paramsByCode[code] ?? [], `${code} no longer sends ${param}`).toContain(param)

      for (const [locale, catalog] of Object.entries(catalogs)) {
        const flat = flattenMessages(catalog)
        const used = new Set<string>()

        for (const key of Object.values(keys)) {
          expect(flat[key], `${locale} missing ${key}`).toBeTruthy()
          for (const match of flat[key].matchAll(/\{(\w+)\}/g)) used.add(match[1])
        }

        expect([...used].sort(), `${locale} variants of ${code}`).toEqual(expected)
      }
    }
  })

  it('gives every CSV code a sentence of its own', () => {
    const csvCodes = codes.filter((code) => code.startsWith('csv_'))
    const keys = csvCodes.map((code) => ERROR_CODE_KEYS[code])

    expect(csvCodes).toHaveLength(16)
    expect(new Set(keys).size).toBe(csvCodes.length)

    for (const locale of LOCALES) {
      setLocale(locale)
      const sentences = keys.map((key) => t(key))

      expect(new Set(sentences).size, locale).toBe(csvCodes.length)
    }
  })

  it('words a problem with one cell the way the import preview words the row', () => {
    const shared = Object.keys(NOTE_CODE_KEYS).filter((code) => Object.hasOwn(ERROR_CODE_KEYS, code))

    expect(shared.sort()).toEqual([
      'csv_amount_overflow',
      'csv_invalid_amount',
      'csv_invalid_date',
      'csv_invalid_type',
      'csv_missing_amount',
      'csv_missing_date',
      'csv_zero_amount',
    ])
    for (const code of shared) {
      expect(ERROR_CODE_KEYS[code], code).toBe(NOTE_CODE_KEYS[code])
    }
  })

  it('says what is wrong with a CSV file, with the value Rust sent', () => {
    expect(commandErrorMessage({ code: 'csv_not_utf8', message: 'raw' })).toBe(
      'That CSV file is not UTF-8 text. Save or export it again as CSV with UTF-8 encoding.',
    )
    expect(
      commandErrorMessage({ code: 'csv_missing_column', message: 'raw', params: { column: 'debit_minor' } }),
    ).toBe(
      'That journal CSV has no “debit_minor” column. Use a file exported from Oikonomia with all its columns.',
    )
    expect(
      commandErrorMessage({ code: 'csv_invalid_date', message: 'raw', params: { value: '31/31/2026' } }),
    ).toBe('"31/31/2026" is not a valid date.')
  })

  it('has a sentence for every mapping problem Rust names, in every language', () => {
    const { param, keys } = VARIANT_KEYS.csv_invalid_mapping

    expect(param).toBe('problem')
    expect(Object.keys(keys)).toEqual(mappingProblems)

    for (const locale of LOCALES) {
      setLocale(locale)
      const shown = mappingProblems.map((problem) =>
        commandErrorMessage({
          code: 'csv_invalid_mapping',
          message: 'raw',
          params: { problem, column: 'Payee' },
        }),
      )

      expect(new Set(shown).size, locale).toBe(mappingProblems.length)
      for (const [index, sentence] of shown.entries()) {
        expect(sentence, locale).not.toContain(mappingProblems[index])
        expect(sentence, locale).not.toMatch(/\{\w+\}/)
        expect(sentence, locale).not.toBe(t('error.csvInvalidMapping'))
        expect(sentence, locale).not.toBe(t('error.unknown'))
      }
    }
  })

  it('names the column a mapping could not find', () => {
    expect(
      commandErrorMessage({
        code: 'csv_invalid_mapping',
        message: 'raw',
        params: { problem: 'unknown_column', column: 'Payee' },
      }),
    ).toBe('The file has no column named “Payee”. Choose one of the file’s columns.')
  })

  it('falls back to the mapping sentence for a problem it cannot word, never the identifier', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    const unusable = [
      { problem: 'a_problem_from_a_newer_version' },
      { problem: 'constructor' },
      // The sentence for an unknown column names the column, which is missing.
      { problem: 'unknown_column' },
      {},
      undefined,
    ]

    for (const locale of LOCALES) {
      setLocale(locale)

      for (const params of unusable) {
        const shown = commandErrorMessage({ code: 'csv_invalid_mapping', message: 'raw', params })

        expect(shown, locale).toBe(t('error.csvInvalidMapping'))
        expect(shown, locale).not.toContain('_')
      }
    }
  })

  it('cuts a long cell from the file and shows markup in it as text', () => {
    const long = 'x'.repeat(FILE_TEXT_LIMIT * 3)
    const cut = commandErrorMessage({ code: 'csv_invalid_status', message: 'raw', params: { value: long } })
    const markup = commandErrorMessage({
      code: 'csv_invalid_status',
      message: 'raw',
      params: { value: '<b>posted</b>' },
    })

    expect(cut).toContain(`“${'x'.repeat(FILE_TEXT_LIMIT)}…”`)
    expect(cut).not.toContain('x'.repeat(FILE_TEXT_LIMIT + 1))
    expect(markup).toContain('“<b>posted</b>”')
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
  const VAGUE_CODES = [
    'database',
    'io',
    'serialization',
    'crypto',
    'unknown',
    'task_failed',
    'app_state_unavailable',
    'validation_internal',
    'analysis',
  ]

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
