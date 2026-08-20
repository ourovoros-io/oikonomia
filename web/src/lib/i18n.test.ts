/** @vitest-environment jsdom */

import { afterEach, describe, expect, test, vi } from 'vitest'
import {
  LOCALE_STORAGE_KEY,
  applyLocale,
  applyLocaleFromPrefs,
  flattenMessages,
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
    expect(flat['tx.header.title']).toBe('Κινήσεις')
    expect(flat['dashboard.eyebrow']).toBe('Επισκόπηση')
    expect(flat['dropzone.title.idle']).toBe(
      'Σύρετε εδώ τιμολόγιο, οφειλή, απόδειξη ή τραπεζικό αντίγραφο',
    )
    expect(flat['accounts.add.label']).toBe('Προσθήκη λογαριασμού')
    expect(flat['tx.form.billStatus.label']).toBe('Κατάσταση λογαριασμού')
    expect(flat['tx.hidden.badge']).toBe('Κρυφή')
    expect(flat['tx.form.hidden.label']).toBe('Απόκρυψη')
    expect(flat['tx.form.hidden.hint']).toBe('Απόκρυψη από την εξαγωγή')
    expect(flat['tx.list.meta.counts']).toBe('{posted} καταχωρισμένα · {hidden} κρυφά')
    expect(flat['tx.export.whisper']).toBe('Η εξαγωγή παραλείπει τις κρυφές γραμμές.')
    expect(flat['quickAdd.hidden.label']).toBe('Κρυφή')
    expect(flat['tx.hidden.action.hide']).toBe('Απόκρυψη')
    expect(flat['tx.hidden.action.show']).toBe('Εμφάνιση')
    expect(flat['tx.hidden.aria.hide']).toBe('Απόκρυψη από την εξαγωγή')
    expect(flat['tx.hidden.aria.show']).toBe('Εμφάνιση στην εξαγωγή')
    expect(flat['tx.export.empty.hidden']).toBe(
      'Δεν υπάρχει τίποτα για εξαγωγή. Όλες οι γραμμές είναι κρυφές.',
    )
    expect(flat['quickAdd.kind.billShort']).toBe('Λογαρ.')
    expect(flat['quickAdd.kind.transferShort']).toBe('Μεταφ.')
    expect(flat['settings.language.title']).toBe('Γλώσσα')
    expect(flat['settings.language.option.en']).toBe('English')
    expect(flat['settings.language.option.el']).toBe('Ελληνικά')
    expect(flat['settings.license.title']).toBe('Άδεια')
    expect(flat['settings.license.description']).toBe(
      'Εισαγάγετε ένα υπογεγραμμένο αρχείο άδειας. Τίποτα δεν αποστέλλεται από αυτόν τον υπολογιστή.',
    )
    expect(flat['settings.license.import']).toBe('Εισαγωγή άδειας')
    expect(flat['settings.license.licensedUntil']).toBe('Άδεια έως {date}')
    expect(flat['settings.license.replace']).toBe('Εισαγωγή άλλου αρχείου')
    expect(flat['settings.license.banner.expired']).toBe(
      'Η άδεια έληξε. Το αντίγραφο θυρίδας, η επαναφορά και η εξαγωγή CSV εξακολουθούν να λειτουργούν.',
    )
    expect(flat['settings.license.error.invalid']).toBe(
      'Αυτό το αρχείο δεν είναι έγκυρη άδεια Oikonomia.',
    )
    expect(flat['settings.license.error.signature']).toBe(
      'Αυτό το αρχείο άδειας δεν είναι σωστά υπογεγραμμένο.',
    )
    expect(flat['settings.license.error.wrongProduct']).toBe(
      'Αυτή η άδεια αφορά διαφορετικό προϊόν.',
    )
    expect(flat['settings.license.error.unreadable']).toBe(
      'Δεν ήταν δυνατή η ανάγνωση του αρχείου άδειας.',
    )
    expect(flat['settings.license.error.generic']).toBe('Δεν ολοκληρώθηκε η εισαγωγή της άδειας.')
    expect(flat['settings.trial.banner.active']).toBe(
      'Απομένουν {n} ημέρες στη δοκιμαστική σας περίοδο',
    )
    expect(flat['settings.trial.banner.expired']).toBe(
      'Η δοκιμαστική περίοδος έληξε. Μπορείτε ακόμη να δημιουργήσετε αντίγραφο της θυρίδας, να κάνετε επαναφορά και να εξαγάγετε CSV.',
    )
    expect(flat['error.core']).toBeUndefined()
    const writerKeys = Object.keys(flat).filter(
      (k) => !k.startsWith('settings.language.') && !k.startsWith('tx.csv.'),
    )
    expect(writerKeys.length).toBe(568)
    expect(flat['tx.csv.import']).toBe('Εισαγωγή CSV')
    expect(flat['tx.csv.kind.expense']).toBe('Έξοδα')
    expect(flat['tx.csv.kind.income']).toBe('Έσοδα')
    expect(flat['tx.csv.kind.transfer']).toBe('Μεταφορά')
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
    expect(t('settings.license.title')).toBe('Άδεια')
    expect(t('settings.license.import')).toBe('Εισαγωγή άδειας')
    expect(t('settings.trial.banner.active', { n: 12 })).toBe(
      'Απομένουν 12 ημέρες στη δοκιμαστική σας περίοδο',
    )
    expect(t('license.entityLimit')).toBe('Απαιτείται άδεια για την προσθήκη άλλου βιβλίου.')
    expect(t('license.entityLimitHint')).toBe(
      'Εισαγάγετε μια υπογεγραμμένη άδεια για περισσότερα από ένα βιβλία σε αυτή τη θυρίδα.',
    )
    expect(t('tx.title')).toBe('Κινήσεις')
    expect(t('dash.overview')).toBe('Επισκόπηση')
    expect(t('acct.addAccount')).toBe('Προσθήκη λογαριασμού')
    expect(t('rpt.title')).toBe('Αναφορές')
    expect(t('rpt.pnl')).toBe('Αποτελέσματα')
    expect(t('docs.title')).toBe('Έγγραφα')
    expect(t('drop.title')).toBe('Σύρετε εδώ τιμολόγιο, οφειλή, απόδειξη ή τραπεζικό αντίγραφο')
    expect(t('tx.billStatus')).toBe('Κατάσταση λογαριασμού')
    expect(t('tx.hidden.badge')).toBe('Κρυφή')
    expect(t('tx.form.hidden.label')).toBe('Απόκρυψη')
    expect(t('tx.form.hidden.hint')).toBe('Απόκρυψη από την εξαγωγή')
    expect(t('tx.export.whisper')).toBe('Η εξαγωγή παραλείπει τις κρυφές γραμμές.')
    expect(t('quickAdd.hidden.label')).toBe('Κρυφή')
    expect(t('tx.csv.import')).toBe('Εισαγωγή CSV')
    expect(t('tx.csv.kind.expense')).toBe('Έξοδα')
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
    expect(t('tx.list.meta.counts', { posted: 24, hidden: 1 })).toBe('24 καταχωρισμένα · 1 κρυφά')
    expect(t('settings.deleteEntityBody', { name: 'Personal' })).toContain('Personal')
  })
})

describe('Writer license catalog', () => {
  const licenseKeys = [
    'settings.license.title',
    'settings.license.description',
    'settings.license.import',
    'settings.license.licensedUntil',
    'settings.license.replace',
    'settings.license.banner.expired',
    'settings.license.error.invalid',
    'settings.license.error.signature',
    'settings.license.error.wrongProduct',
    'settings.license.error.unreadable',
    'settings.license.error.generic',
    'settings.trial.banner.active',
    'settings.trial.banner.expired',
    'license.entityLimit',
    'license.entityLimitHint',
  ] as const

  test('EL keys exist in both en and el catalogs', () => {
    const enFlat = flattenMessages(en)
    const elFlat = flattenMessages(el)
    for (const key of licenseKeys) {
      expect(enFlat[key]).toBeTruthy()
      expect(elFlat[key]).toBeTruthy()
    }
    expect(enFlat['settings.license.title']).toBe('License')
    expect(elFlat['settings.license.title']).toBe('Άδεια')
    expect(enFlat['settings.license.error.generic']).toBe('Could not import the license.')
    expect(elFlat['settings.license.error.generic']).toBe(
      'Δεν ολοκληρώθηκε η εισαγωγή της άδειας.',
    )
    expect(enFlat['license.entityLimit']).toBe(
      'A license is required to add another book.',
    )
    expect(elFlat['license.entityLimit']).toBe(
      'Απαιτείται άδεια για την προσθήκη άλλου βιβλίου.',
    )
    expect(enFlat['license.entityLimitHint']).toBe(
      'Import a signed license to keep more than one book in this vault.',
    )
    expect(elFlat['license.entityLimitHint']).toBe(
      'Εισαγάγετε μια υπογεγραμμένη άδεια για περισσότερα από ένα βιβλία σε αυτή τη θυρίδα.',
    )
  })
})

describe('t fallback', () => {
  test('missing el key falls back to English or the key', () => {
    setLocale('el')
    expect(t('__missing.page.key__')).toBe('__missing.page.key__')
  })

  test('en.json key missing or empty in el.json falls back to English', () => {
    setLocale('el')
    expect(t('kind.other')).toBe(en['kind.other'])
    expect(t('tx.hidden.badge')).toBe('Κρυφή')

    setLocaleMessagesForTests('el', { 'kind.other': '   ' })
    expect(t('kind.other')).toBe(en['kind.other'])
  })

  test('interpolates {name} placeholders in English', () => {
    expect(t('settings.deleteEntityBody', { name: 'Personal' })).toContain('Personal')
  })
})

describe('setLocale persist', () => {
  test('mirrors oikonomia.locale and calls persist', () => {
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
    expect(t('__missing.page.key__')).toBe('__missing.page.key__')
  })
})
