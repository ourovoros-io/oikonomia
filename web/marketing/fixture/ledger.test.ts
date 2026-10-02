import { describe, expect, test } from 'vitest'
import { accountDefaults, buildLedger, DEMO_TODAY } from './ledger'

describe('demo ledger', () => {
  const en = buildLedger('en')
  const el = buildLedger('el')

  test('every entry balances', () => {
    for (const view of en.entries) {
      const debits = view.lines.reduce((sum, l) => sum + l.debit.amount_minor, 0)
      const credits = view.lines.reduce((sum, l) => sum + l.credit.amount_minor, 0)

      expect(debits, view.entry.description).toBe(credits)
      expect(debits).toBeGreaterThan(0)
    }
  })

  test('nothing is dated after the frozen day', () => {
    for (const view of en.entries) {
      expect(view.entry.entry_date <= DEMO_TODAY, view.entry.entry_date).toBe(true)
    }
  })

  test('covers June through September 2026', () => {
    const months = new Set(en.entries.map((v) => v.entry.entry_date.slice(0, 7)))

    expect([...months].sort()).toEqual(['2026-06', '2026-07', '2026-08', '2026-09'])
  })

  test('same structure in both languages, translated names', () => {
    expect(el.entries.length).toBe(en.entries.length)
    expect(el.accounts.map((a) => a.code)).toEqual(en.accounts.map((a) => a.code))
    expect(el.accounts.find((a) => a.code === '5020')?.name).toBe('Ενοίκιο εργαστηρίου')
    expect(en.accounts.find((a) => a.code === '5020')?.name).toBe('Studio rent')
    expect(el.entity.name).toBe('Εργαστήριο Νεφελόρα')
  })

  test('lines only reference known accounts', () => {
    const ids = new Set(en.accounts.map((a) => a.id))

    for (const view of en.entries) {
      for (const line of view.lines) {
        expect(ids.has(line.account_id), line.account_id).toBe(true)
      }
    }
  })

  test('documents point at real entries and use unix-seconds created_at', () => {
    const entryIds = new Set(en.entries.map((v) => v.entry.id))
    const frozen = new Date(`${DEMO_TODAY}T23:59:59`).getTime() / 1000

    expect(en.documents.length).toBeGreaterThanOrEqual(4)

    for (const doc of en.documents) {
      expect(entryIds.has(doc.entry_id)).toBe(true)
      expect(doc.created_at).toMatch(/^\d+$/)
      expect(Number(doc.created_at)).toBeLessThanOrEqual(frozen)
    }
  })

  test('every default account exists in the chart, with the type its role needs', () => {
    const roleTypes = {
      category: 'expense',
      payment: 'asset',
      deposit: 'asset',
      income: 'income',
      bill_category: 'expense',
      bills_payable: 'liability',
      transfer_source: 'asset',
      transfer_destination: 'asset',
    } as const

    for (const [role, type] of Object.entries(roleTypes)) {
      const id = accountDefaults()[role as keyof typeof roleTypes]
      const found = en.accounts.find((a) => a.id === id)

      expect(found?.account_type, role).toBe(type)
    }
  })
})
