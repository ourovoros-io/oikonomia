/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import {
  LOCALE_STORAGE_KEY,
  applyLocale,
  applyLocaleFromPrefs,
  getLocale,
  parseLocale,
  readCachedLocale,
  resetI18nForTests,
  setLocale,
  setLocaleMessagesForTests,
  setLocalePersist,
  t,
} from './i18n'
import en from '../locales/en.json' with { type: 'json' }

afterEach(() => {
  resetI18nForTests()
})

describe('parseLocale', () => {
  test('accepts en and el', () => {
    expect(parseLocale('en')).toBe('en')
    expect(parseLocale('el')).toBe('el')
  })

  test('invalid values become en', () => {
    expect(parseLocale('fr')).toBe('en')
    expect(parseLocale('')).toBe('en')
    expect(parseLocale(undefined)).toBe('en')
    expect(parseLocale(1)).toBe('en')
  })
})

describe('t fallback', () => {
  test('missing el key falls back to English', () => {
    setLocale('el')
    expect(t('unlock.titleWelcome')).toBe(en['unlock.titleWelcome'])
  })

  test('empty el key falls back to English', () => {
    setLocale('el')
    expect(t('unlock.titleCreate')).toBe(en['unlock.titleCreate'])
    expect(t('settings.title')).toBe(en['settings.title'])
  })

  test('interpolates {name} placeholders', () => {
    expect(t('settings.deleteEntityBody', { name: 'Personal' })).toContain('Personal')
  })
})

describe('setLocale persist', () => {
  test('writes optimistic cache and calls persist', () => {
    const persist = vi.fn()
    setLocalePersist(persist)
    setLocale('el')
    expect(getLocale()).toBe('el')
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe('el')
    expect(persist).toHaveBeenCalledWith('el')
  })

  test('reload hydrates from UiPrefs, not the cache, as source of truth', () => {
    setLocale('el')
    expect(readCachedLocale()).toBe('el')

    resetI18nForTests()
    expect(getLocale()).toBe('en')

    applyLocaleFromPrefs({ locale: 'el' })
    expect(getLocale()).toBe('el')
  })

  test('invalid cached value becomes en', () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, 'de')
    expect(readCachedLocale()).toBe('en')
  })

  test('absent prefs.locale defaults to en', () => {
    applyLocale('el')
    applyLocaleFromPrefs({})
    expect(getLocale()).toBe('en')
  })
})

describe('test catalog overlay', () => {
  test('stub el value is used for lookup', () => {
    setLocaleMessagesForTests('el', { 'unlock.titleCreate': 'EL Create your vault' })
    setLocale('el')
    expect(t('unlock.titleCreate')).toBe('EL Create your vault')
    expect(t('unlock.titleWelcome')).toBe(en['unlock.titleWelcome'])
  })
})
