/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      dashboardSummary: vi.fn(),
      entryList: vi.fn(),
      accountList: vi.fn(),
    },
  }
})

import type { Account, DashboardSummary, Entity, PostedEntryView } from '../lib/api'
import { api } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { DashboardPage } from './DashboardPage'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

describe('DashboardPage empty-state CTA', () => {
  test('no-book empty state renders Create a book and calls onCreateBook', async () => {
    const onCreateBook = vi.fn()
    render(<DashboardPage entity={null} onCreateBook={onCreateBook} />)
    const cta = screen.getByRole('button', { name: 'Create a book' })
    await userEvent.click(cta)
    expect(onCreateBook).toHaveBeenCalledTimes(1)
  })

  test('renders without a CTA when onCreateBook is not supplied', () => {
    render(<DashboardPage entity={null} />)
    expect(screen.queryByRole('button', { name: 'Create a book' })).toBeNull()
  })
})

describe('DashboardPage activity row colours', () => {
  const entity: Entity = {
    id: 'e1',
    name: 'Personal',
    base_currency: 'EUR',
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
    cash_like_assets: 0,
    income: 120000,
    expenses: 4250,
    net_income: 115750,
    recent_entry_count: 2,
  }

  const entries: PostedEntryView[] = [
    {
      entry: {
        id: 'j1',
        entity_id: 'e1',
        entry_date: '2026-08-12',
        description: 'Alpha supermarket',
        reference: null,
        status: 'posted',
        hidden: false,
      },
      lines: [
        {
          id: 'l1',
          entry_id: 'j1',
          account_id: 'exp1',
          debit: { amount_minor: 4250 },
          credit: { amount_minor: 0 },
          memo: null,
        },
        {
          id: 'l2',
          entry_id: 'j1',
          account_id: 'w1',
          debit: { amount_minor: 0 },
          credit: { amount_minor: 4250 },
          memo: null,
        },
      ],
      is_voided: false,
    },
    {
      entry: {
        id: 'j2',
        entity_id: 'e1',
        entry_date: '2026-08-11',
        description: 'Client invoice',
        reference: null,
        status: 'posted',
        hidden: false,
      },
      lines: [
        {
          id: 'l3',
          entry_id: 'j2',
          account_id: 'w1',
          debit: { amount_minor: 120000 },
          credit: { amount_minor: 0 },
          memo: null,
        },
        {
          id: 'l4',
          entry_id: 'j2',
          account_id: 'inc1',
          debit: { amount_minor: 0 },
          credit: { amount_minor: 120000 },
          memo: null,
        },
      ],
      is_voided: false,
    },
  ]

  test('income and expense activity rows wear the Ledger money tones, never status colours', async () => {
    vi.mocked(api.dashboardSummary).mockReset().mockResolvedValue(summary)
    vi.mocked(api.entryList).mockReset().mockResolvedValue(entries)
    vi.mocked(api.accountList).mockReset().mockResolvedValue(accounts)

    render(<DashboardPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByText('Client invoice')).toBeTruthy()
    })

    const expenseBadge = screen.getByText('Alpha supermarket').closest('li')?.firstElementChild
    expect(expenseBadge?.className).toContain('bg-[var(--color-money-out-soft)]')

    const incomeBadge = screen.getByText('Client invoice').closest('li')?.firstElementChild
    expect(incomeBadge?.className).toContain('bg-[var(--color-money-in-soft)]')
  })
})
