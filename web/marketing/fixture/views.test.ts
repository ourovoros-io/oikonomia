import { describe, expect, test } from 'vitest'
import { buildLedger } from './ledger'
import {
  accountBalance,
  accountRegister,
  balanceSheet,
  cashFlowSeries,
  dashboardSummary,
  entryList,
  pnl,
  previousWindow,
  trialBalance,
} from './views'

const l = buildLedger('en')
const SEPT = ['2026-09-01', '2026-09-30'] as const

describe('derived views agree with each other', () => {
  test('trial balance balances', () => {
    const tb = trialBalance(l, '2026-09-24')

    expect(tb.total_debits).toBe(tb.total_credits)
  })

  test('P&L net is income minus expenses, and September is non-trivial', () => {
    const p = pnl(l, ...SEPT)

    expect(p.net_income).toBe(p.total_income - p.total_expenses)
    expect(p.total_income).toBeGreaterThan(0)
    expect(p.total_expenses).toBeGreaterThan(0)
  })

  test('dashboard month matches the P&L month', () => {
    const d = dashboardSummary(l, ...SEPT, '2026-09-24')
    const p = pnl(l, ...SEPT)

    expect(d.income).toBe(p.total_income)
    expect(d.expenses).toBe(p.total_expenses)
    expect(d.net_income).toBe(p.net_income)
    expect(d.top_expense?.code).toBe('5020')
    expect(d.cash_like_assets).toBe(
      accountBalance(l, 'acc-1000', '2026-09-24') + accountBalance(l, 'acc-1020', '2026-09-24'),
    )
  })

  test('cash-flow totals match the P&L and buckets are daily for a month', () => {
    const c = cashFlowSeries(l, ...SEPT)
    const p = pnl(l, ...SEPT)

    expect(c.granularity).toBe('day')
    expect(c.buckets).toHaveLength(30)
    expect(c.total_income_minor).toBe(p.total_income)
    expect(c.total_expenses_minor).toBe(p.total_expenses)
    expect(c.buckets.at(-1)?.cumulative_income_minor).toBe(p.total_income)
  })

  test('null bounds resolve to the first and last entry, monthly', () => {
    const c = cashFlowSeries(l, null, null)

    expect(c.from).toBe('2026-06-01')
    expect(c.to).toBe('2026-09-23')
    expect(c.granularity).toBe('month')
  })

  test('balance sheet balances once earnings sit in equity', () => {
    const bs = balanceSheet(l, '2026-09-24')

    expect(bs.total_assets).toBe(bs.total_liabilities_equity)
  })

  test('entry list filters and sorts newest first', () => {
    const rows = entryList(l, { from: '2026-09-01', to: '2026-09-30', search: 'market', accountId: null })

    expect(rows.length).toBeGreaterThan(0)
    expect(rows.every((r) => r.entry.description.toLowerCase().includes('market'))).toBe(true)
    expect(rows[0].entry.entry_date >= rows.at(-1)!.entry.entry_date).toBe(true)
  })

  test('previous window for whole calendar month uses month boundaries', () => {
    const prev = previousWindow('2026-09-01', '2026-09-30')

    expect(prev.from).toBe('2026-08-01')
    expect(prev.to).toBe('2026-08-31')
  })

  test('account register seeds with prior balance when from is set', () => {
    const priorBalance = accountBalance(l, 'acc-1020', '2026-08-31')
    const register = accountRegister(l, 'acc-1020', '2026-09-01', '2026-09-30')

    expect(register.length).toBeGreaterThan(0)

    // First line's balance should equal prior balance plus the first line's signed movement
    const first = register[0]!
    const movement = first.debit_minor - first.credit_minor
    const expectedBalance = priorBalance + movement

    expect(first.balance_minor).toBe(expectedBalance)
  })
})
