/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
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
    },
  }
})

// jsdom has no 2D canvas; the light's painting is tested on its own.
vi.mock('../components/CashFlowPulse', () => ({
  CashFlowPulse: ({ label }: { label: string }) => <div role="img" aria-label={label} />,
}))

import type { CashFlowSeries, DashboardSummary, Entity, PostedEntryView } from '../lib/api'
import { api } from '../lib/api'
import { applyLocale, resetI18nForTests } from '../lib/i18n'
import { formatMoney } from '../lib/money'
import { DashboardPage } from './DashboardPage'

const EUR = { code: 'EUR', decimals: 2 }

const money = (minor: number, signed = false) =>
  formatMoney(minor, EUR, 'en', { signed })
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
  hidden_entry_count: 0,
}

const series: CashFlowSeries = {
  entity_id: 'e1',
  from: '2026-08-01',
  to: '2026-08-31',
  granularity: 'day',
  total_income_minor: 120000,
  total_expenses_minor: 4250,
  net_minor: 115750,
  hidden_entry_count: 0,
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
    listed_kind: 'expense',
    simple_form: null,
  },
  {
    entry: { id: 'j2', entity_id: 'e1', entry_date: '2026-08-11', description: 'Client invoice', reference: null, status: 'posted', hidden: false },
    lines: [line('l3', 'j2', 'w1', 120000, 0), line('l4', 'j2', 'inc1', 0, 120000)],
    is_voided: false,
    listed_kind: 'income',
    simple_form: null,
  },
]

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(new Date(2026, 7, 15, 12))
  vi.mocked(api.dashboardSummary).mockReset().mockResolvedValue(summary)
  vi.mocked(api.cashFlowSeries).mockReset().mockResolvedValue(series)
  vi.mocked(api.entryList).mockReset().mockResolvedValue(entries)
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

describe('DashboardPage activity kinds', () => {
  test('bills are labelled Bill as in the entry list, whatever their lines look like', async () => {
    const [expense, income] = entries as [PostedEntryView, PostedEntryView]
    const bill = (id: string, description: string, lines: PostedEntryView['lines']): PostedEntryView => ({
      ...expense,
      entry: { ...expense.entry, id, description },
      lines,
      listed_kind: 'bill',
      simple_form: null,
    })
    vi.mocked(api.entryList).mockResolvedValue([
      // Lines of an expense, and of a transfer from the wallet to a liability.
      bill('b1', 'Power paid now', expense.lines),
      bill('b2', 'Water paid off', [line('l5', 'b2', 'p1', 900, 0), line('l6', 'b2', 'w1', 0, 900)]),
      { ...income, entry: { ...income.entry, id: 't1', description: 'To savings' }, listed_kind: 'transfer' },
    ])
    render(<DashboardPage entity={entity} />)

    const row = async (description: string) => (await screen.findByText(description)).closest('li')
    for (const [description, minor] of [['Power paid now', 4250], ['Water paid off', 900]] as const) {
      const billRow = await row(description)
      expect(billRow).toHaveTextContent('Bill')
      expect(billRow).not.toHaveTextContent(/Expense|Transfer/)
      expect(billRow?.firstElementChild?.className).toContain('bg-[var(--color-money-out-soft)]')
      expect(billRow?.querySelector('[data-amount]')).toHaveAttribute('data-amount', 'out')
      expect(plain(billRow?.textContent ?? '')).toContain(plain(money(-minor, true)))
    }
    const transfer = await row('To savings')
    expect(transfer).toHaveTextContent('Transfer')
    expect(transfer?.querySelector('[data-amount]')).toHaveAttribute('data-amount', 'neutral')
  })

  test('only an entry core lists as an opening balance is labelled one', async () => {
    const [, income] = entries as [PostedEntryView, PostedEntryView]
    vi.mocked(api.entryList).mockResolvedValue([
      { ...income, entry: { ...income.entry, id: 'o1', description: 'Checking start' }, listed_kind: 'opening_balance' },
      { ...income, entry: { ...income.entry, id: 'j1', description: 'Owner pays in' }, listed_kind: 'other' },
    ])
    render(<DashboardPage entity={entity} />)

    const opening = (await screen.findByText('Checking start')).closest('li')
    expect(opening).toHaveTextContent('Opening balance')
    expect(opening?.querySelector('svg.lucide-flag')).not.toBeNull()
    expect(opening?.querySelector('[data-amount]')).toHaveAttribute('data-amount', 'neutral')
    const journal = screen.getByText('Owner pays in').closest('li')
    expect(journal).toHaveTextContent('Journal entry')
    expect(journal).not.toHaveTextContent('Opening balance')
    expect(journal?.querySelector('svg.lucide-notebook-pen')).not.toBeNull()
    expect(journal?.querySelector('[data-amount]')).toHaveAttribute('data-amount', 'neutral')
  })
})

describe('DashboardPage activity dates', () => {
  test('a language change rewrites the dates already shown', async () => {
    render(<DashboardPage entity={entity} />)
    const row = await screen.findByText('Alpha supermarket')
    const dateLine = () => row.closest('li')?.querySelector('.tabular-nums')?.firstChild

    expect(dateLine()?.textContent).toBe('12/08/2026')

    act(() => applyLocale('de'))

    expect(dateLine()?.textContent).toBe('12.08.2026')
  })
})

describe('DashboardPage hidden entries', () => {
  test('says how many hidden entries the net includes, using the count from core', async () => {
    vi.mocked(api.dashboardSummary).mockResolvedValue({ ...summary, hidden_entry_count: 2 })
    render(<DashboardPage entity={entity} />)

    expect(
      await screen.findByText('Includes 2 hidden entries, which are omitted from exports.'),
    ).toBeInTheDocument()
  })

  test('says nothing when no hidden entry is included', async () => {
    render(<DashboardPage entity={entity} />)

    await screen.findByRole('heading', { level: 1, name: 'Personal' })
    expect(screen.queryByText(/hidden entr/)).toBeNull()
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
