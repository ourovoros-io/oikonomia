/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      dashboardSummary: vi.fn(),
      cashFlowSeries: vi.fn(),
      entryList: vi.fn(),
      accountList: vi.fn(),
    },
  }
})

// jsdom has no 2D canvas; the light's painting is tested on its own.
vi.mock('../components/CashFlowPulse', () => ({
  CashFlowPulse: ({ label }: { label: string }) => <div role="img" aria-label={label} />,
}))

import type { Account, CashFlowSeries, DashboardSummary, Entity, PostedEntryView } from '../lib/api'
import { api } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { formatMoney, localeForCurrency } from '../lib/money'
import { DashboardPage } from './DashboardPage'

const EUR = { code: 'EUR', decimals: 2 }

const money = (minor: number, signed = false) =>
  formatMoney(minor, EUR, localeForCurrency('EUR'), { signed })
/** Rendered money carries a no-break space; text matchers see a plain one. */
const plain = (text: string) => text.replace(/\s/g, ' ')

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  base_currency_decimals: 2,
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

function account(over: Partial<Account> & Pick<Account, 'id' | 'name' | 'account_type'>): Account {
  return {
    entity_id: 'e1',
    code: '1000',
    parent_id: null,
    is_active: true,
    is_system: false,
    sort_order: 0,
    ...over,
  }
}

const accounts: Account[] = [
  account({ id: 'w1', name: 'Checking', account_type: 'asset' }),
  account({ id: 'exp1', name: 'Groceries', account_type: 'expense' }),
  account({ id: 'inc1', name: 'Salary', account_type: 'income' }),
]

const summary: DashboardSummary = {
  entity_id: 'e1',
  base_currency: 'EUR',
  cash_like_assets: 98765,
  income: 120000,
  expenses: 4250,
  net_income: 115750,
  recent_entry_count: 2,
  savings_rate_bps: 9646,
  spend_ratio_bps: 354,
  top_expense: { code: '5100', name: 'Groceries', amount_minor: 4250, share_bps: 10000 },
  net_vs_previous_bps: null,
}

const series: CashFlowSeries = {
  entity_id: 'e1',
  from: '2026-08-01',
  to: '2026-08-31',
  granularity: 'day',
  total_income_minor: 120000,
  total_expenses_minor: 4250,
  net_minor: 115750,
  buckets: [],
}

function line(id: string, entryId: string, accountId: string, debit: number, credit: number) {
  return {
    id,
    entry_id: entryId,
    account_id: accountId,
    debit: { amount_minor: debit },
    credit: { amount_minor: credit },
    memo: null,
  }
}

const entries: PostedEntryView[] = [
  {
    entry: { id: 'j1', entity_id: 'e1', entry_date: '2026-08-12', description: 'Alpha supermarket', reference: null, status: 'posted', hidden: false },
    lines: [line('l1', 'j1', 'exp1', 4250, 0), line('l2', 'j1', 'w1', 0, 4250)],
    is_voided: false,
  },
  {
    entry: { id: 'j2', entity_id: 'e1', entry_date: '2026-08-11', description: 'Client invoice', reference: null, status: 'posted', hidden: false },
    lines: [line('l3', 'j2', 'w1', 120000, 0), line('l4', 'j2', 'inc1', 0, 120000)],
    is_voided: false,
  },
]

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date(2026, 7, 15, 12))
  vi.mocked(api.dashboardSummary).mockReset().mockResolvedValue(summary)
  vi.mocked(api.cashFlowSeries).mockReset().mockResolvedValue(series)
  vi.mocked(api.entryList).mockReset().mockResolvedValue(entries)
  vi.mocked(api.accountList).mockReset().mockResolvedValue(accounts)
})

afterEach(() => {
  cleanup()
  resetI18nForTests()
  vi.useRealTimers()
})

describe('DashboardPage empty-state CTA', () => {
  test('no-book empty state renders Create a book and calls onCreateBook', async () => {
    const onCreateBook = vi.fn()
    render(<DashboardPage entity={null} onCreateBook={onCreateBook} />)
    await userEvent.click(screen.getByRole('button', { name: 'Create a book' }))
    expect(onCreateBook).toHaveBeenCalledTimes(1)
  })

  test('renders without a CTA when onCreateBook is not supplied', () => {
    render(<DashboardPage entity={null} />)
    expect(screen.queryByRole('button', { name: 'Create a book' })).toBeNull()
  })
})

describe('DashboardPage activity row colours', () => {
  test('income and expense activity rows wear the Ledger money tones, never status colours', async () => {
    render(<DashboardPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByText('Client invoice')).toBeTruthy()
    })

    const expenseBadge = screen.getByText('Alpha supermarket').closest('li')?.firstElementChild
    expect(expenseBadge?.className).toContain('bg-[var(--color-money-out-soft)]')
    const incomeBadge = screen.getByText('Client invoice').closest('li')?.firstElementChild
    expect(incomeBadge?.className).toContain('bg-[var(--color-money-in-soft)]')

    expect(screen.getByText('Alpha supermarket').closest('li')?.querySelector('[data-amount]')).toHaveAttribute('data-amount', 'out')
    expect(screen.getByText('Client invoice').closest('li')?.querySelector('[data-amount]')).toHaveAttribute('data-amount', 'in')
  })
})

describe('DashboardPage hero and arcs', () => {
  test('the top bar names the book and the period', async () => {
    render(<DashboardPage entity={entity} />)

    expect(await screen.findByRole('heading', { level: 1, name: 'Personal' })).toBeInTheDocument()
    expect(screen.getByText('August 2026 · EUR')).toBeInTheDocument()
  })

  test('the net is drawn as light, with in, out and assets beside it', async () => {
    render(<DashboardPage entity={entity} />)

    expect(
      await screen.findByRole('img', {
        name: `Money in ${money(120000)}, money out ${money(4250)}, net ${money(115750, true)}, August 2026.`,
      }),
    ).toBeInTheDocument()
    const figure = document.querySelector('[data-net]')
    expect(figure).toHaveAttribute('data-net', 'in')
    expect(figure).toHaveTextContent(plain(money(115750, true)))
    expect(document.querySelector('[data-money-pill="in"]')).toHaveTextContent(plain(`In ${money(120000)}`))
    expect(document.querySelector('[data-money-pill="out"]')).toHaveTextContent(plain(`Out ${money(4250)}`))
    expect(document.querySelector('[data-money-pill="neutral"]')).toHaveTextContent(plain(`Assets ${money(98765)}`))
    expect(screen.getByText('96.5% of everything that came in, kept.')).toBeInTheDocument()
  })

  test('arc tiles read their basis points, and a missing value says so', async () => {
    render(<DashboardPage entity={entity} />)

    await waitFor(() => {
      expect(screen.getByRole('meter', { name: 'Savings rate' })).toHaveAttribute('aria-valuetext', '96.5%')
    })
    expect(screen.getByRole('meter', { name: 'Spend ratio' })).toHaveAttribute('aria-valuetext', '3.5%')
    expect(screen.getByRole('meter', { name: 'Net vs last period' })).toHaveAttribute('aria-valuetext', 'No value yet')
    const top = screen.getByRole('meter', { name: 'Top spend' })
    expect(top).toHaveAttribute('aria-valuetext', '100.0%')
    expect(top).toHaveAccessibleDescription('Groceries')
    expect(screen.getByText('2 entries this month')).toBeInTheDocument()
  })

  test('Quarter asks Rust for the calendar quarter', async () => {
    render(<DashboardPage entity={entity} />)
    await waitFor(() => {
      expect(api.dashboardSummary).toHaveBeenCalledWith('e1', '2026-08-01', '2026-08-31', '2026-08-15')
    })

    await userEvent.click(screen.getByRole('button', { name: 'Quarter' }))

    await waitFor(() => {
      expect(api.dashboardSummary).toHaveBeenLastCalledWith('e1', '2026-07-01', '2026-09-30', '2026-08-15')
    })
    expect(api.cashFlowSeries).toHaveBeenLastCalledWith('e1', '2026-07-01', '2026-09-30')
    expect(screen.getByText('Q3 2026 · EUR')).toBeInTheDocument()
    expect(screen.getByRole('meter', { name: 'Net vs last period' })).toHaveAccessibleDescription('Against last quarter')
  })

  test('a loss wears the out light and claims nothing was kept', async () => {
    vi.mocked(api.dashboardSummary).mockResolvedValue({
      ...summary,
      income: 1000,
      expenses: 1500,
      net_income: -500,
      savings_rate_bps: -5000,
      spend_ratio_bps: 15000,
    })
    render(<DashboardPage entity={entity} />)

    await waitFor(() => {
      expect(document.querySelector('[data-net]')).toHaveAttribute('data-net', 'out')
    })
    expect(screen.queryByText(/of everything that came in/)).toBeNull()
  })
})
