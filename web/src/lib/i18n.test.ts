/** @vitest-environment jsdom */

import { afterEach, describe, expect, test } from 'vitest'
import {
  LOCALE_STORAGE_KEY,
  applyLocale,
  flattenMessages,
  getLocale,
  hydrateLocaleFromStorage,
  parseLocale,
  readCachedLocale,
  resetI18nForTests,
  setLocale,
  setLocaleMessagesForTests,
  t,
} from './i18n'
import en from '../locales/en.json' with { type: 'json' }
import el from '../locales/el.json' with { type: 'json' }

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

describe('Writer el catalog', () => {
  test('nested catalog flattens to Writer paths plus language chrome', () => {
    const flat = flattenMessages(el)
    expect(flat['app.nav.dashboard']).toBe('Επισκόπηση')
    expect(flat['unlock.title.unlock']).toBe('Καλωσορίσατε')
    expect(flat['settings.language.title']).toBe('Γλώσσα')
    expect(flat['settings.language.english']).toBe('English')
    expect(flat['settings.language.greek']).toBe('Ελληνικά')
    expect(Object.keys(flat).length).toBeGreaterThanOrEqual(174)
  })

  test('t() uses Writer wording via current extract keys', () => {
    setLocale('el')
    expect(t('nav.dashboard')).toBe('Επισκόπηση')
    expect(t('unlock.titleWelcome')).toBe('Καλωσορίσατε')
    expect(t('unlock.titleCreate')).toBe('Δημιουργία θυρίδας')
    expect(t('settings.title')).toBe('Ρυθμίσεις')
    expect(t('settings.vaultBackup.restore')).toBe('Επαναφορά')
    expect(t('settings.vaultBackup.backupVault')).toBe('Αντίγραφο θυρίδας')
    expect(t('settings.reencrypting')).toBe('Επανακρυπτογράφηση…')
    expect(t('kind.billShort')).toBe('Λογαρ.')
    expect(t('kind.transferShort')).toBe('Μεταφ.')
    expect(t('quickAdd.due')).toBe('Οφειλή')
    expect(t('quickAdd.paid')).toBe('Εξοφλημένο')
    expect(t('quickAdd.vaultLocked')).toBe('Η θυρίδα είναι κλειδωμένη')
    expect(t('quickAdd.createBookFirst')).toBe('Δημιουργήστε πρώτα βιβλίο')
    expect(t('settings.language.title')).toBe('Γλώσσα')
    expect(t('settings.language.description')).toBe('Μενού, ετικέτες και Γρήγορη καταχώριση.')
  })

  test('interpolates Writer placeholders from extract var names', () => {
    setLocale('el')
    expect(t('settings.entities.nBooks', { count: 3 })).toBe('3 βιβλία')
    expect(t('app.entityChart', { currency: 'EUR', chart: 'προσωπικό' })).toBe(
      'EUR · λογιστικό σχέδιο προσωπικό',
    )
    expect(t('quickAdd.saved', { kind: 'Έξοδο', money: '€12.00' })).toBe(
      'Αποθηκεύτηκε Έξοδο €12.00',
    )
    expect(t('quickAdd.amount', { ccy: 'EUR' })).toBe('Ποσό (EUR)')
    expect(t('settings.deleteEntityBody', { name: 'Personal' })).toContain('Personal')
  })
})

describe('t fallback', () => {
  test('missing el key falls back to English', () => {
    setLocale('el')
    expect(t('dash.overview')).toBe(en['dash.overview'])
    expect(t('tx.title')).toBe(en['tx.title'])
  })

  test('interpolates {name} placeholders in English', () => {
    expect(t('settings.deleteEntityBody', { name: 'Personal' })).toContain('Personal')
  })
})

describe('setLocale persist', () => {
  test('writes oikonomia.locale only', () => {
    setLocale('el')
    expect(getLocale()).toBe('el')
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe('el')
  })

  test('reload hydrates from oikonomia.locale', () => {
    setLocale('el')
    expect(readCachedLocale()).toBe('el')

    resetI18nForTests()
    expect(getLocale()).toBe('en')
    localStorage.setItem(LOCALE_STORAGE_KEY, 'el')

    expect(hydrateLocaleFromStorage()).toBe('el')
    expect(getLocale()).toBe('el')
  })

  test('invalid stored value becomes en', () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, 'de')
    expect(readCachedLocale()).toBe('en')
    expect(hydrateLocaleFromStorage()).toBe('en')
  })

  test('missing key defaults to en', () => {
    applyLocale('el')
    resetI18nForTests()
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBeNull()
    expect(hydrateLocaleFromStorage()).toBe('en')
    expect(getLocale()).toBe('en')
  })
})

describe('test catalog overlay', () => {
  test('stub el value is used for lookup', () => {
    setLocaleMessagesForTests('el', { 'unlock.titleCreate': 'EL Create your vault' })
    setLocale('el')
    expect(t('unlock.titleCreate')).toBe('EL Create your vault')
    expect(t('dash.overview')).toBe(en['dash.overview'])
  })
})
