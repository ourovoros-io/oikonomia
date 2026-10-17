import { afterEach, describe, expect, test, vi } from 'vitest'

import { resetI18nForTests, setLocale, t } from './i18n'
import { commandFailure, copyError, notesError, uiErrorText } from './uiError'

afterEach(() => {
  vi.restoreAllMocks()
  resetI18nForTests()
})

describe('uiErrorText', () => {
  test('words a catalog sentence in the language on screen when it is asked', () => {
    const error = copyError('settings.entities.restoreNameTaken', { name: 'Home' })
    const english = uiErrorText(error)

    setLocale('el')

    expect(english).toContain('Home')
    expect(uiErrorText(error)).toBe(t('settings.entities.restoreNameTaken', { name: 'Home' }))
    expect(uiErrorText(error)).not.toBe(english)
  })

  test('words a failed command by its code, with the screen sentence for a vague one', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(uiErrorText(commandFailure({ code: 'vault_locked', message: 'raw' }))).toBe(
      t('error.vaultLocked'),
    )
    expect(uiErrorText(commandFailure({ code: 'io', message: 'raw' }, 'docs.deleteFailed'))).toBe(
      t('docs.deleteFailed'),
    )
  })

  test('words analysis notes, or the fallback when none can be worded', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(uiErrorText(notesError([{ code: 'no_text_extracted' }], null, 'tx.couldNotReadDoc'))).toBe(
      t('analyze.notes.noText'),
    )
    expect(uiErrorText(notesError([{ code: 'brand_new' }], null, 'tx.couldNotReadDoc'))).toBe(
      t('tx.couldNotReadDoc'),
    )
  })
})

describe('notesError', () => {
  test('logs a note it cannot word once, when built; wording it again never logs', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    const error = notesError([{ code: 'brand_new' }], null, 'tx.couldNotReadDoc')
    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn.mock.calls[0][0]).toContain('brand_new')

    uiErrorText(error)
    setLocale('fr')
    uiErrorText(error)

    expect(warn).toHaveBeenCalledTimes(1)
  })
})

describe('commandFailure', () => {
  test('logs a hidden cause once, when caught; wording it again never logs', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    const error = commandFailure({ code: 'io', message: 'EACCES /Users/x' }, 'docs.deleteFailed')
    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn.mock.calls[0][0]).toContain('EACCES /Users/x')

    uiErrorText(error)
    setLocale('de')
    uiErrorText(error)

    expect(warn).toHaveBeenCalledTimes(1)
  })

  test('does not log an error the person can fix', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    commandFailure({ code: 'name_taken', message: 'raw', params: { name: 'Home' } })

    expect(warn).not.toHaveBeenCalled()
  })
})
