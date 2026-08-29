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
import fr from '../locales/fr.json' with { type: 'json' }
import de from '../locales/de.json' with { type: 'json' }

afterEach(() => {
  resetI18nForTests()
})

describe('parseLocale', () => {
  test('accepts en and el', () => {
    expect(parseLocale('en')).toBe('en')
    expect(parseLocale('el')).toBe('el')
  })

  test('accepts-fr-and-de', () => {
    expect(parseLocale('fr')).toBe('fr')
    expect(parseLocale('de')).toBe('de')
  })

  test('invalid values become en', () => {
    expect(parseLocale('xx')).toBe('en')
    expect(parseLocale('')).toBe('en')
    expect(parseLocale(undefined)).toBe('en')
    expect(parseLocale(1)).toBe('en')
  })
})

describe('Writer fr and de catalogs', () => {
  const sampleKeys = [
    'nav.dashboard',
    'unlock.titleWelcome',
    'drop.title',
    'settings.license.title',
    'settings.language.title',
  ] as const

  test('flatten(fr) key-set equals flatten(de) equals flatten(el)', () => {
    const elKeys = new Set(Object.keys(flattenMessages(el)))
    const frKeys = new Set(Object.keys(flattenMessages(fr)))
    const deKeys = new Set(Object.keys(flattenMessages(de)))
    expect(frKeys).toEqual(elKeys)
    expect(deKeys).toEqual(elKeys)
  })

  test('t() in fr and de is not English and not the raw key', () => {
    const english = Object.fromEntries(sampleKeys.map((key) => [key, t(key)]))
    for (const locale of ['fr', 'de'] as const) {
      setLocale(locale)
      for (const key of sampleKeys) {
        const value = t(key)
        expect(value, `${locale} ${key}`).not.toBe(english[key])
        expect(value, `${locale} ${key}`).not.toBe(key)
      }
    }
  })

  test('catalogs do not mention AI, IA, or KI', () => {
    const catalogs = { fr: JSON.stringify(fr), de: JSON.stringify(de) }
    const forbidden = [
      /\bAI\b/,
      /\bIA\b/,
      /\bKI\b/,
      /intelligence artificielle/i,
      /künstliche Intelligenz/i,
      /KI-Modell/,
    ]
    for (const [name, text] of Object.entries(catalogs)) {
      for (const pattern of forbidden) {
        expect(text, `${name} ${pattern}`).not.toMatch(pattern)
      }
    }
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
    expect(flat['settings.language.option.fr']).toBe('Français')
    expect(flat['settings.language.option.de']).toBe('Deutsch')
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

describe('Writer recurring catalog', () => {
  const recurringKeys = [
    'tx.recurring',
    'recurring.title',
    'recurring.templates',
    'recurring.subtitle',
    'recurring.whisper',
    'recurring.new',
    'recurring.backToEntries',
    'recurring.emptyTitle',
    'recurring.emptyBody',
    'recurring.templatesCount',
    'recurring.dueCount',
    'recurring.due',
    'recurring.post',
    'recurring.postConfirm.title',
    'recurring.postConfirm.body',
    'recurring.postConfirm.confirm',
    'recurring.postConfirm.cancel',
    'recurring.form.titleNew',
    'recurring.form.titleEdit',
    'recurring.form.kind',
    'recurring.form.name',
    'recurring.form.namePlaceholder',
    'recurring.form.amount',
    'recurring.form.cadence',
    'recurring.form.dayOfMonth',
    'recurring.form.dayOfMonthHint',
    'recurring.form.category',
    'recurring.form.fromAccount',
    'recurring.form.memo',
    'recurring.form.memoPlaceholder',
    'recurring.form.save',
    'recurring.form.cancel',
    'recurring.form.busy',
    'recurring.form.error',
    'recurring.delete.title',
    'recurring.delete.body',
    'recurring.delete.confirm',
    'recurring.cadence.monthly',
    'recurring.cadence.weekly',
    'recurring.cadence.yearly',
    'recurring.posting.busy',
    'recurring.posting.error',
    'recurring.form.cadenceMonthly',
    'recurring.form.cadenceWeekly',
    'recurring.form.cadenceYearly',
    'tx.form.kind.expense',
    'tx.form.kind.income',
    'tx.form.kind.bill',
    'tx.form.kind.transfer',
  ] as const

  test('EN and EL keys exist for critical Recurring chrome', () => {
    const enFlat = flattenMessages(en)
    const elFlat = flattenMessages(el)
    for (const key of recurringKeys) {
      expect(enFlat[key], key).toBeTruthy()
      expect(elFlat[key], key).toBeTruthy()
    }
    expect(enFlat['tx.recurring']).toBe('Recurring')
    expect(elFlat['tx.recurring']).toBe('Επαναλαμβανόμενα')
    expect(enFlat['recurring.title']).toBe('Recurring')
    expect(elFlat['recurring.title']).toBe('Επαναλαμβανόμενα')
    expect(enFlat['recurring.templates']).toBe('Templates')
    expect(elFlat['recurring.templates']).toBe('Πρότυπα')
    expect(enFlat['recurring.subtitle']).toBe('A lightweight recipe — not a second ledger.')
    expect(elFlat['recurring.subtitle']).toBe('Μια ελαφριά συνταγή — όχι δεύτερο ημερολόγιο.')
    expect(enFlat['recurring.whisper']).toBe('local only')
    expect(elFlat['recurring.whisper']).toBe('μόνο τοπικά')
    expect(enFlat['recurring.emptyTitle']).toBe('No recurring templates yet')
    expect(elFlat['recurring.emptyTitle']).toBe('Δεν υπάρχουν επαναλαμβανόμενα πρότυπα ακόμη')
    expect(enFlat['recurring.post']).toBe('Post')
    expect(elFlat['recurring.post']).toBe('Καταχώριση')
    expect(enFlat['recurring.postConfirm.title']).toBe('Post {name}?')
    expect(elFlat['recurring.postConfirm.title']).toBe('Καταχώριση του «{name}»;')
    expect(enFlat['recurring.form.cadenceMonthly']).toBe('Monthly')
    expect(elFlat['recurring.form.cadenceMonthly']).toBe('Μηνιαία')
    expect(enFlat['tx.form.kind.expense']).toBe('Expense')
    expect(elFlat['tx.form.kind.expense']).toBe('Έξοδο')
    expect(elFlat['recurring.templatesCount']).toContain('{n}')
    expect(elFlat['recurring.dueCount']).toContain('{n}')
    expect(elFlat['recurring.title']).not.toMatch(/Oikonomia/i)
    expect(enFlat['recurring.title']).toBe('Recurring')
  })

  test('t() interpolates template and due counts', () => {
    expect(t('recurring.templatesCount', { n: 0 })).toBe('0 templates')
    expect(t('recurring.dueCount', { n: 2 })).toBe('2 due')
    setLocale('el')
    expect(t('tx.recurring')).toBe('Επαναλαμβανόμενα')
    expect(t('recurring.templatesCount', { n: 0 })).toBe('0 πρότυπα')
    expect(t('recurring.postConfirm.title', { name: 'Rent' })).toBe(
      'Καταχώριση του «Rent»;',
    )
  })
})

describe('Writer monthly-expense PDF catalog', () => {
  const pdfKeys = [
    'reports.exportPdf',
    'reports.pdf.title',
    'reports.pdf.meta',
    'reports.pdf.totalExpenses',
    'reports.pdf.sliceNote',
    'reports.pdf.emptyTitle',
    'reports.pdf.emptyBody',
    'reports.pdf.footerPrivacy',
    'reports.pdf.footerLocal',
    'reports.pdf.other',
    'reports.pdf.busy',
    'reports.pdf.error',
  ] as const

  test('EN and EL keys exist with Writer copy', () => {
    const enFlat = flattenMessages(en)
    const elFlat = flattenMessages(el)
    for (const key of pdfKeys) {
      expect(enFlat[key]).toBeTruthy()
      expect(elFlat[key]).toBeTruthy()
    }
    expect(enFlat['reports.exportPdf']).toBe('Export PDF')
    expect(elFlat['reports.exportPdf']).toBe('Εξαγωγή PDF')
    expect(enFlat['reports.pdf.title']).toBe('Monthly expenses')
    expect(elFlat['reports.pdf.title']).toBe('Μηνιαία έξοδα')
    expect(enFlat['reports.pdf.meta']).toBe(
      'Expense categories · amounts in {currency} · Hidden entries omitted',
    )
    expect(elFlat['reports.pdf.meta']).toBe(
      'Κατηγορίες εξόδων · ποσά σε {currency} · Οι κρυφές γραμμές παραλείπονται',
    )
    expect(enFlat['reports.pdf.sliceNote']).toBe(
      'Top 6 categories by amount; remaining categories folded into Other…',
    )
    expect(elFlat['reports.pdf.sliceNote']).toBe(
      'Οι 6 μεγαλύτερες κατηγορίες· οι υπόλοιπες στο Λοιπά…',
    )
    expect(enFlat['reports.pdf.emptyBody']).toContain('{period}')
    expect(elFlat['reports.pdf.emptyBody']).toBe(
      'Δεν υπάρχει τίποτα για διάγραμμα στην περίοδο {period}. Αλλάξτε τις ημερομηνίες της αναφοράς ή προσθέστε έξοδα στις Κινήσεις.',
    )
    expect(elFlat['reports.pdf.footerPrivacy']).toBe(
      'Τα βιβλία σας δεν φεύγουν ποτέ από αυτόν τον υπολογιστή',
    )
    expect(elFlat['reports.pdf.footerLocal']).toContain('Oikonomia')
    expect(enFlat['reports.pdf.busy']).toBe('Exporting…')
    expect(elFlat['reports.pdf.busy']).toBe('Εξαγωγή…')
    expect(enFlat['reports.pdf.error']).toBe('Could not export the PDF.')
    expect(elFlat['reports.pdf.error']).toBe('Δεν ολοκληρώθηκε η εξαγωγή PDF.')
    expect(enFlat['reports.pdf.export']).toBeUndefined()
    expect(elFlat['reports.pdf.export']).toBeUndefined()
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

describe('Writer unlock.update catalog', () => {
  const updateKeys = [
    'unlock.update.button',
    'unlock.update.dialogTitle',
    'unlock.update.checking.title',
    'unlock.update.checking.body',
    'unlock.update.upToDate.title',
    'unlock.update.upToDate.body',
    'unlock.update.available.title',
    'unlock.update.available.version',
    'unlock.update.available.notesLabel',
    'unlock.update.available.size',
    'unlock.update.available.honesty',
    'unlock.update.available.confirm',
    'unlock.update.failed.title',
    'unlock.update.failed.body',
    'unlock.update.installing.title',
    'unlock.update.installing.body',
    'unlock.update.cancel',
    'unlock.update.close',
  ] as const

  test('keys exist in en, el, fr, and de with Writer copy', () => {
    const catalogs = {
      en: flattenMessages(en),
      el: flattenMessages(el),
      fr: flattenMessages(fr),
      de: flattenMessages(de),
    }
    for (const [name, flat] of Object.entries(catalogs)) {
      for (const key of updateKeys) {
        expect(flat[key], `${name} ${key}`).toBeTruthy()
      }
    }

    expect(catalogs.en['unlock.update.button']).toBe('Check for update')
    expect(catalogs.el['unlock.update.button']).toBe('Έλεγχος ενημέρωσης')
    expect(catalogs.fr['unlock.update.button']).toBe('Rechercher une mise à jour')
    expect(catalogs.de['unlock.update.button']).toBe('Nach Update suchen')

    expect(catalogs.en['unlock.update.upToDate.title']).toBe('You’re up to date')
    expect(catalogs.en['unlock.update.failed.title']).toBe('Couldn’t check')
    expect(catalogs.fr['unlock.update.checking.body']).toBe(
      'Recherche d’une nouvelle application.',
    )
    expect(catalogs.fr['unlock.update.upToDate.body']).toBe('C’est le dernier Oikonomia.')
    expect(catalogs.fr['unlock.update.upToDate.title']).toBe('Vous êtes à jour')
    expect(catalogs.en['unlock.update.available.honesty']).toBe(
      'This is the only internet contact, and only to fetch a new application.',
    )
    expect(catalogs.en['unlock.update.available.version']).toBe('Oikonomia {version}')
    expect(catalogs.el['unlock.update.available.version']).toContain('Oikonomia')
    expect(catalogs.fr['unlock.update.available.version']).toContain('Oikonomia')
    expect(catalogs.de['unlock.update.available.version']).toContain('Oikonomia')
    expect(catalogs.en['unlock.update.available.notesLabel']).toBe('What’s new')
    expect(catalogs.en['unlock.update.available.size']).toBe('{size}')
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

  test('fr and de persist to cache and the writer', () => {
    const persist = vi.fn()
    setLocalePersist(persist)
    setLocale('fr')
    expect(getLocale()).toBe('fr')
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe('fr')
    expect(persist).toHaveBeenCalledWith('fr')
    setLocale('de')
    expect(getLocale()).toBe('de')
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe('de')
    expect(persist).toHaveBeenCalledWith('de')
  })

  test('reload hydrates from UiPrefs, not the cache, as source of truth', () => {
    setLocale('el')
    expect(readCachedLocale()).toBe('el')

    resetI18nForTests()
    expect(getLocale()).toBe('en')

    applyLocaleFromPrefs({ locale: 'el' })
    expect(getLocale()).toBe('el')

    applyLocaleFromPrefs({ locale: 'fr' })
    expect(getLocale()).toBe('fr')
    applyLocaleFromPrefs({ locale: 'de' })
    expect(getLocale()).toBe('de')
  })

  test('invalid cached value becomes en', () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, 'xx')
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
