import { describe, expect, test } from 'vitest'
import { flattenMessages } from './i18n'
import en from '../locales/en.json' with { type: 'json' }
import el from '../locales/el.json' with { type: 'json' }
import fr from '../locales/fr.json' with { type: 'json' }
import de from '../locales/de.json' with { type: 'json' }

const catalogs = {
  en: flattenMessages(en),
  el: flattenMessages(el),
  fr: flattenMessages(fr),
  de: flattenMessages(de),
}

/** The 2026-10-08 QA review (chapters 3 and 5) settled these terms; keep each concept to one word. */
describe('settled wording', () => {
  test('Greek tells the Bill tab from the Accounts item', () => {
    expect(catalogs.el['tx.form.kind.bill']).toBe('Τιμολόγιο')
    expect(catalogs.el['tx.form.kind.bill']).not.toBe(catalogs.el['app.nav.accounts'])
    expect(catalogs.el['kind.bill']).not.toBe(catalogs.el['app.nav.accounts'])
  })

  test('German calls ledger rows Buchungen on every screen', () => {
    const stray = Object.entries(catalogs.de).filter(([, text]) => /Bewegungen/u.test(text))

    expect(stray).toEqual([])
  })

  test('French does not keep the English "vs"', () => {
    const stray = Object.entries(catalogs.fr).filter(([, text]) => /\bvs\b/u.test(text))

    expect(stray).toEqual([])
  })

  test('the archive and restore messages say book, never entity, and end with a full stop', () => {
    const keys = [
      'settings.entities.archiveError',
      'settings.entities.archived.loadError',
      'settings.entities.restoreError',
    ]

    for (const locale of ['en', 'el', 'fr', 'de'] as const) {
      for (const key of keys) {
        const text = catalogs[locale][key]
        if (text === undefined) continue
        expect(text, `${locale} ${key}`).toMatch(/\.$/u)
        expect(text, `${locale} ${key}`).not.toMatch(/entity|entité|οντότητ|Entität/iu)
      }
    }
  })

  test('no English string says entity or entities: the user-facing term is book', () => {
    const stray = Object.entries(catalogs.en).filter(([, text]) => /\bentit(y|ies)\b/iu.test(text))

    expect(stray).toEqual([])
  })

  test('no el, fr or de string uses the word for entity either', () => {
    const word = /οντότητ|\bentités?\b|\bEntitäten?\b/iu
    const stray = (['el', 'fr', 'de'] as const).flatMap((locale) =>
      Object.entries(catalogs[locale])
        .filter(([, text]) => word.test(text))
        .map(([key]) => `${locale} ${key}`),
    )

    expect(stray).toEqual([])
  })

  test('every English "Could not" message ends with a full stop', () => {
    const missing = Object.entries(catalogs.en).filter(
      ([, text]) => /^Could not/u.test(text) && !/\.$/u.test(text),
    )

    expect(missing).toEqual([])
  })

  test('the delete-book message ends with a full stop in every language', () => {
    const keys = {
      en: 'settings.deleteFailed',
      el: 'settings.entities.deleteError',
      fr: 'settings.entities.deleteError',
      de: 'settings.entities.deleteError',
    } as const

    for (const [locale, key] of Object.entries(keys) as [keyof typeof keys, string][]) {
      expect(catalogs[locale][key], `${locale} ${key}`).toMatch(/\.$/u)
    }
  })

  test('Greek failure messages use "Δεν ήταν δυνατή", not the headline "Αποτυχία"', () => {
    const stray = Object.entries(catalogs.el).filter(([, text]) => /^Αποτυχία\b/u.test(text))

    expect(stray).toEqual([])
  })

  test('French puts a narrow no-break space, not a plain one, before ; ? and !', () => {
    const plain = Object.entries(catalogs.fr).filter(([, text]) => / [;?!]/u.test(text))

    expect(plain).toEqual([])
  })
})
