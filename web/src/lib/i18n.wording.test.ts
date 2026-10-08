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

  test('French puts a narrow no-break space, not a plain one, before ; ? and !', () => {
    const plain = Object.entries(catalogs.fr).filter(([, text]) => / [;?!]/u.test(text))

    expect(plain).toEqual([])
  })
})
