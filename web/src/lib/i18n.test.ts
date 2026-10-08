/** @vitest-environment jsdom */

import { afterEach, describe, expect, test, vi } from 'vitest'
import {
  LOCALE_STORAGE_KEY,
  applyLocale,
  flattenMessages,
  getLocale,
  parseLocale,
  readCachedLocale,
  resetI18nForTests,
  resolvesInLocale,
  setLocale,
  setLocaleMessagesForTests,
  setLocalePersist,
  t,
  tn,
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
    'settings.title',
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
    expect(flat['quickAdd.kind.billShort']).toBe('Λογαρ.')
    expect(flat['quickAdd.kind.transferShort']).toBe('Μεταφ.')
    expect(flat['settings.language.title']).toBe('Γλώσσα')
    expect(flat['settings.language.option.en']).toBe('English')
    expect(flat['settings.language.option.el']).toBe('Ελληνικά')
    expect(flat['settings.language.option.fr']).toBe('Français')
    expect(flat['settings.language.option.de']).toBe('Deutsch')
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
    expect(t('settings.language.description')).toBe('Μενού, ετικέτες και Γρήγορη καταχώριση. Τα νέα βιβλία και οι νέες αυτόματες καταχωρίσεις χρησιμοποιούν αυτή τη γλώσσα· ό,τι υπάρχει ήδη δεν αλλάζει.')
    expect(t('tx.title')).toBe('Κινήσεις')
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
    expect(enFlat['recurring.subtitle']).toBe('A lightweight template — not a second ledger.')
    expect(elFlat['recurring.subtitle']).toBe('Ένα ελαφρύ πρότυπο — όχι δεύτερο ημερολόγιο.')
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
    expect(elFlat['recurring.templatesCount.other']).toContain('{n}')
    expect(elFlat['recurring.dueCount.other']).toContain('{n}')
    expect(elFlat['recurring.title']).not.toMatch(/Oikonomia/i)
    expect(enFlat['recurring.title']).toBe('Recurring')
  })

  test('tn() picks the singular and the plural form by the language rules', () => {
    expect(tn('recurring.templatesCount', 0)).toBe('0 templates')
    expect(tn('recurring.templatesCount', 1)).toBe('1 template')
    expect(tn('recurring.dueCount', 2)).toBe('2 due')
    expect(tn('dash.entriesThis', 1, { period: 'month' })).toBe('1 entry this month')
    expect(tn('dash.entriesThis', 5, { period: 'month' })).toBe('5 entries this month')
    setLocale('fr')
    expect(tn('recurring.templatesCount', 0)).toBe('0 modèle')
    expect(tn('recurring.templatesCount', 2)).toBe('2 modèles')
  })

  test('t() interpolates template and due counts', () => {
    setLocale('el')
    expect(t('tx.recurring')).toBe('Επαναλαμβανόμενα')
    expect(tn('recurring.templatesCount', 3)).toBe('3 πρότυπα')
    expect(tn('recurring.templatesCount', 1)).toBe('1 πρότυπο')
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

describe('Writer unlock.update catalog', () => {
  const updateKeys = [
    'unlock.update.button',
    'unlock.update.checking.title',
    'unlock.update.checking.body',
    'unlock.update.upToDate.title',
    'unlock.update.upToDate.body',
    'unlock.update.available.title',
    'unlock.update.available.version',
    'unlock.update.available.honesty',
    'unlock.update.available.confirm',
    'unlock.update.availableManually.note',
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

    expect(catalogs.en['unlock.update.button']).toBe('Check for updates')
    expect(catalogs.el['unlock.update.button']).toBe('Έλεγχος ενημέρωσης')
    expect(catalogs.fr['unlock.update.button']).toBe('Rechercher une mise à jour')
    expect(catalogs.de['unlock.update.button']).toBe('Nach Update suchen')

    expect(catalogs.en['unlock.update.upToDate.title']).toBe('You’re up to date')
    expect(catalogs.en['unlock.update.failed.title']).toBe('Couldn’t check')
    expect(catalogs.fr['unlock.update.checking.body']).toBe(
      'Recherche d’une version plus récente.',
    )
    expect(catalogs.fr['unlock.update.upToDate.body']).toBe(
      'Vous disposez de la dernière version d’Oikonomia.',
    )
    expect(catalogs.fr['unlock.update.upToDate.title']).toBe('Vous êtes à jour')
    expect(catalogs.en['unlock.update.available.honesty']).toBe(
      'This is the only time Oikonomia connects to the internet, and only to download an update.',
    )
    expect(catalogs.en['unlock.update.available.version']).toBe('Oikonomia {version}')
    expect(catalogs.el['unlock.update.available.version']).toContain('Oikonomia')
    expect(catalogs.fr['unlock.update.available.version']).toContain('Oikonomia')
    expect(catalogs.de['unlock.update.available.version']).toContain('Oikonomia')
  })
})

describe('t fallback', () => {
  test('missing el key falls back to English or the key', () => {
    setLocale('el')
    expect(t('__missing.page.key__')).toBe('__missing.page.key__')
  })

  test('en.json key missing or empty in el.json falls back to English', () => {
    setLocale('el')
    // A key that exists only in the English test overlay (never in the el
    // catalog) still resolves, via the en fallback in t().
    setLocaleMessagesForTests('en', { '__test.onlyEnglish__': 'Only in English' })
    expect(t('__test.onlyEnglish__')).toBe('Only in English')
    expect(t('tx.hidden.badge')).toBe('Κρυφή')

    // An empty/whitespace-only el override for that same key is treated as
    // absent, so t() still falls back to the English overlay.
    setLocaleMessagesForTests('el', { '__test.onlyEnglish__': '   ' })
    expect(t('__test.onlyEnglish__')).toBe('Only in English')
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
  })

  test('invalid cached value becomes en', () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, 'xx')
    expect(readCachedLocale()).toBe('en')
  })
})

describe('audited English-identical leftovers are genuinely correct', () => {
  test('fr CSV field/column labels are real French words, properly cased', () => {
    const flat = flattenMessages(fr)
    // "Date", "Description", "Type", and "Note" are unmodified French
    // vocabulary in this context (loanwords with identical spelling), not
    // untranslated leftovers.
    expect(flat['tx.csv.field.date']).toBe('Date')
    expect(flat['tx.csv.field.description']).toBe('Description')
    expect(flat['tx.csv.col.type']).toBe('Type')
    expect(flat['tx.csv.col.note']).toBe('Note')
  })

  test('de recurring form "Name" is the correct German word, not a leftover', () => {
    expect(flattenMessages(de)['recurring.form.name']).toBe('Name')
  })

  test('de "November" is the correct full German month name, matching the other 11', () => {
    const flat = flattenMessages(de)
    // date.month.* renders as "{month} {year}" in DateInput's calendar
    // header; November is genuinely spelled the same in German, so
    // abbreviating only this one month would be the inconsistent choice.
    expect(flat['date.month.11']).toBe('November')
    expect(flat['date.month.10']).toBe('Oktober')
    expect(flat['date.month.12']).toBe('Dezember')
  })
})

describe('locale parity guard', () => {
  test('every english catalog key resolves in every locale', () => {
    const englishKeys = Object.keys(flattenMessages(en))
    for (const locale of ['el', 'fr', 'de'] as const) {
      const missing = englishKeys.filter((key) => !resolvesInLocale(locale, key))
      expect(missing, `${locale} silently falls back for: ${missing.join(', ')}`).toEqual([])
    }
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
