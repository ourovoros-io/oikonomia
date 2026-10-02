import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, test } from 'vitest'
import { CURRENCIES } from './currencies'
import { KEY_ALIASES, REVERSE_ALIASES, flattenMessages } from './i18n'
import en from '../locales/en.json' with { type: 'json' }
import el from '../locales/el.json' with { type: 'json' }
import fr from '../locales/fr.json' with { type: 'json' }
import de from '../locales/de.json' with { type: 'json' }

/**
 * TEMPORARY. Nested catalog keys that the Rust-text work (tasks 2 to 6 of
 * .superpowers/sdd/i18n-rust-text/plan.md) will either start reading, or
 * delete once Rust owns the text. Until then nothing reads them and English
 * has no twin for them, so the two guards below would fail on them.
 *
 * Later work empties this list, and it must end empty: the final task
 * deletes it together with its exemptions. Do not add keys to it.
 */
const PENDING_RUST: readonly string[] = [
  // Chart-of-accounts names: move into the Rust text table (task 6).
  'coa.personal.1000',
  'coa.personal.1010',
  'coa.personal.1020',
  'coa.personal.1100',
  'coa.personal.2000',
  'coa.personal.2050',
  'coa.personal.2100',
  'coa.personal.3000',
  'coa.personal.3100',
  'coa.personal.4000',
  'coa.personal.4100',
  'coa.personal.4200',
  'coa.personal.4900',
  'coa.personal.5000',
  'coa.personal.5100',
  'coa.personal.5200',
  'coa.personal.5300',
  'coa.personal.5350',
  'coa.personal.5400',
  'coa.personal.5500',
  'coa.personal.5600',
  'coa.personal.5700',
  'coa.personal.5900',
  'coa.company.1010',
  'coa.company.1100',
  'coa.company.1500',
  'coa.company.2000',
  'coa.company.2100',
  'coa.company.2200',
  'coa.company.2300',
  'coa.company.3100',
  'coa.company.3200',
  'coa.company.4000',
  'coa.company.5000',
  'coa.company.5100',
  'coa.company.5200',
  'coa.company.5300',
  'coa.company.5400',
  'coa.company.5500',
  'coa.company.5600',
  'coa.company.5900',

  // Opening-balance and suggested-description text: move into Rust (task 6).
  'tx.entry.openingBalance',
  'analyze.invoice.merchant.naturalGas',
  'analyze.invoice.merchant.electricity',
  'analyze.invoice.desc.invoiceRefMerchant',
  'analyze.invoice.desc.invoiceRef',
  'analyze.invoice.desc.invoice',

  // Analyzer hints and notes: rendered from coded Rust text (task 4).
  'analyze.hint.ready',
  'analyze.hint.missingModels',
  'analyze.notes.noText',
  'analyze.notes.amountCurrency',
  'analyze.notes.datedFromDoc',
  'analyze.notes.addPayable',
  'analyze.notes.pdfBudget',
  'analyze.notes.parsedText',
  'analyze.notes.ocrPathMissing',
  'analyze.notes.ocrModelsMissing',
  'analyze.notes.ocrRead',
  'analyze.notes.ocrLittleText',
  'analyze.notes.ocrError',
  'analyze.notes.ocrPdfImage',
  'analyze.invoice.notes.parsed',
  'analyze.invoice.notes.noTotal',
  'analyze.invoice.notes.income',
  'analyze.invoice.notes.utility',
  'analyze.invoice.notes.unpaid',
  'analyze.invoice.notes.vatExempt',

  // Synthetic report rows (task 4).
  'reports.synthetic.retainedEarnings',
  'reports.synthetic.netIncome',
]

/**
 * Keys that production code builds at run time, as template literals. Each
 * family lists its values in full, so a new value must be added here on
 * purpose and a typo cannot hide a dead key behind a wildcard.
 */
const DYNAMIC_KEY_FAMILIES: Record<string, readonly string[]> = {
  // TransactionsPage, DashboardPage and QuickAddApp: t(`kind.${kind}`)
  'kind.*': ['expense', 'income', 'bill', 'transfer', 'other'].map((v) => `kind.${v}`),
  // App and SettingsPage: t(`chart.${chart_template}`)
  'chart.*': ['personal', 'company', 'blank'].map((v) => `chart.${v}`),
  // DateInput: t(`date.month.${month}`), month 1 to 12
  'date.month.*': Array.from({ length: 12 }, (_, i) => `date.month.${i + 1}`),
  // SettingsPage: t(`settings.language.option.${locale}`)
  'settings.language.option.*': ['en', 'el', 'fr', 'de'].map(
    (v) => `settings.language.option.${v}`,
  ),
  // SettingsPage: t(`currency.${code}`), enumerated from the picker's own list
  'currency.*': CURRENCIES.map((c) => `currency.${c.code}`),
  // DashboardPage: t(`dashboard.period.word.${period}`)
  'dashboard.period.word.*': ['month', 'quarter', 'year'].map(
    (v) => `dashboard.period.word.${v}`,
  ),
  // DashboardPage: t(`dashboard.arc.previous.hint.${period}`)
  'dashboard.arc.previous.hint.*': ['month', 'quarter', 'year'].map(
    (v) => `dashboard.arc.previous.hint.${v}`,
  ),
  // recurring.ts kindLabelKey: `tx.form.kind.${kind}`
  'tx.form.kind.*': ['expense', 'income', 'bill', 'transfer'].map((v) => `tx.form.kind.${v}`),
}

const SRC_ROOT = fileURLToPath(new URL('..', import.meta.url))

const catalogs = {
  en: flattenMessages(en),
  el: flattenMessages(el),
  fr: flattenMessages(fr),
  de: flattenMessages(de),
}

const NESTED_LOCALES = ['el', 'fr', 'de'] as const

const pending = new Set(PENDING_RUST)

const enKeys = Object.keys(catalogs.en)

/**
 * Every catalog key an English key can be stored or read under: itself, its
 * nested alias, and any nested path that REVERSE_ALIASES sends back to it
 * (Writer paths that production code uses directly, such as the chart-template
 * titles in SettingsPage).
 */
function spellings(key: string): string[] {
  const reversed = Object.entries(REVERSE_ALIASES)
    .filter(([, english]) => english === key)
    .map(([nested]) => nested)

  return [key, KEY_ALIASES[key], ...reversed].filter(
    (spelling): spelling is string => spelling !== undefined,
  )
}

/** The catalog key a locale actually stores `key` under, if any. */
function twin(locale: keyof typeof catalogs, key: string): string | undefined {
  return spellings(key).find((spelling) => catalogs[locale][spelling] !== undefined)
}

function isPending(key: string): boolean {
  return spellings(key).some((spelling) => pending.has(spelling))
}

/** Production sources only: no tests, no type stubs, no catalogs, not i18n.ts. */
function productionSources(dir: string = SRC_ROOT): string[] {
  const files: string[] = []

  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = `${dir}${entry.name}`

    if (entry.isDirectory()) {
      if (entry.name === 'locales') continue
      files.push(...productionSources(`${path}/`))
      continue
    }

    if (!/\.tsx?$/.test(entry.name)) continue
    if (/\.test\.tsx?$/.test(entry.name) || entry.name.endsWith('.d.ts')) continue
    if (path === `${SRC_ROOT}lib/i18n.ts`) continue

    files.push(path)
  }

  return files
}

/** Every quoted string that looks like a dotted catalog key. */
function literalKeys(): Set<string> {
  const found = new Set<string>()
  const quoted = /(['"`])([A-Za-z][\w-]*(?:\.[\w-]+)*)\1/g

  for (const file of productionSources()) {
    for (const match of readFileSync(file, 'utf8').matchAll(quoted)) {
      found.add(match[2])
    }
  }

  return found
}

/** Placeholder names, with the pairs `expandVars` bridges folded together. */
function placeholders(text: string): string[] {
  const canonical: Record<string, string> = {
    n: 'count',
    ccy: 'currency',
    template: 'chart',
    money: 'amount',
  }
  const names = [...text.matchAll(/\{(\w+)\}/g)].map((m) => canonical[m[1]] ?? m[1])

  return [...new Set(names)].sort()
}

describe('catalog readers', () => {
  test('every enumerated dynamic key resolves in English', () => {
    // Some families are Writer paths, which English reaches through an alias.
    const resolves = (key: string) =>
      [key, KEY_ALIASES[key], REVERSE_ALIASES[key]].some(
        (spelling) => spelling !== undefined && catalogs.en[spelling] !== undefined,
      )

    const stale = Object.values(DYNAMIC_KEY_FAMILIES)
      .flat()
      .filter((key) => !resolves(key))

    expect(stale).toEqual([])
  })

  test('every hand-written reverse alias is read by production code', () => {
    // A reverse alias that merely mirrors a key alias is generated. One that
    // does not exists only because production code uses the Writer path.
    const read = literalKeys()

    const unread = Object.entries(REVERSE_ALIASES)
      .filter(([nested, english]) => KEY_ALIASES[english] !== nested)
      .map(([nested]) => nested)
      .filter((nested) => !read.has(nested))

    expect(unread).toEqual([])
  })

  test('every English key is read by production code', () => {
    const read = new Set([...literalKeys(), ...Object.values(DYNAMIC_KEY_FAMILIES).flat()])

    const unread = enKeys.filter(
      (key) => !isPending(key) && !spellings(key).some((spelling) => read.has(spelling)),
    )

    expect(unread).toEqual([])
  })
})

describe('catalog parity', () => {
  test('every pending key exists in the nested catalogs', () => {
    expect(PENDING_RUST.filter((key) => catalogs.el[key] === undefined)).toEqual([])
  })

  test('el, fr and de have the same keys', () => {
    const elKeys = Object.keys(catalogs.el).sort()

    expect(Object.keys(catalogs.fr).sort()).toEqual(elKeys)
    expect(Object.keys(catalogs.de).sort()).toEqual(elKeys)
  })

  test('every alias source exists in English and every target in el', () => {
    const badSources = Object.keys(KEY_ALIASES).filter((key) => catalogs.en[key] === undefined)
    const badTargets = Object.values(KEY_ALIASES).filter((key) => catalogs.el[key] === undefined)

    expect(badSources).toEqual([])
    expect(badTargets).toEqual([])
  })

  test('every reverse alias maps an el key to an English key', () => {
    const badSources = Object.keys(REVERSE_ALIASES).filter((key) => catalogs.el[key] === undefined)
    const badTargets = Object.values(REVERSE_ALIASES).filter(
      (key) => catalogs.en[key] === undefined,
    )

    expect(badSources).toEqual([])
    expect(badTargets).toEqual([])
  })

  test('every English key has a translation in each nested locale', () => {
    const missing = NESTED_LOCALES.flatMap((locale) =>
      enKeys
        .filter((key) => !isPending(key) && twin(locale, key) === undefined)
        .map((key) => `${locale}: ${key}`),
    )

    expect(missing).toEqual([])
  })

  test('no nested key lacks an English counterpart', () => {
    const reachable = new Set(enKeys.flatMap((key) => spellings(key)))

    const orphans = Object.keys(catalogs.el).filter(
      (key) => !reachable.has(key) && !pending.has(key),
    )

    expect(orphans).toEqual([])
  })

  test('every key has the same placeholders in all four languages', () => {
    const mismatched: string[] = []

    for (const key of enKeys) {
      if (isPending(key)) continue

      const english = placeholders(catalogs.en[key])

      for (const locale of NESTED_LOCALES) {
        const stored = twin(locale, key)
        if (stored === undefined) continue

        const other = placeholders(catalogs[locale][stored])
        if (other.join() !== english.join()) {
          mismatched.push(`${locale} ${key}: en {${english}} vs {${other}}`)
        }
      }
    }

    expect(mismatched).toEqual([])
  })
})
