/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { Account, AccountDefaults, Entity, RecurringTemplate } from '../lib/api'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      recurringList: vi.fn(),
      recurringCreate: vi.fn(),
      recurringUpdate: vi.fn(),
      recurringDelete: vi.fn(),
      recurringPost: vi.fn(),
      accountList: vi.fn(),
      accountDefaults: vi.fn(),
    },
  }
})

import { api } from '../lib/api'
import { resetI18nForTests, setLocale } from '../lib/i18n'
import { RecurringPage } from './RecurringPage'

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
    is_active: true,
    is_system: false,
    sort_order: 0,
    ...over,
  }
}

const accounts: Account[] = [
  account({ id: 'w1', name: 'Checking', account_type: 'asset', code: '1000' }),
  account({ id: 'exp1', name: 'Housing', account_type: 'expense', code: '5000' }),
  account({ id: 'inc1', name: 'Salary', account_type: 'income', code: '4000' }),
]

function template(over: Partial<RecurringTemplate> = {}): RecurringTemplate {
  return {
    id: 'r1',
    entity_id: 'e1',
    name: 'Rent',
    kind: 'expense',
    bill_status: null,
    amount_minor: 85000,
    cadence: 'monthly',
    day_of_month: 1,
    category_account_id: 'exp1',
    wallet_account_id: 'w1',
    payable_account_id: null,
    from_account_id: null,
    to_account_id: null,
    memo: null,
    next_date: '2026-08-01',
    due: true,
    ...over,
  }
}

const rent = template()
const payroll = template({
  id: 'r2',
  name: 'Payroll',
  kind: 'income',
  amount_minor: 120000,
  due: false,
  next_date: '2026-09-01',
  category_account_id: 'inc1',
})

const DEFAULTS: AccountDefaults = {
  category: 'exp1',
  payment: 'w1',
  deposit: 'w1',
  income: 'inc1',
  bill_category: 'exp1',
  bills_payable: null,
  transfer_source: 'w1',
  transfer_destination: 'w1',
}

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(api.recurringList).mockReset().mockResolvedValue([])
  vi.mocked(api.recurringCreate).mockReset().mockResolvedValue(rent)
  vi.mocked(api.recurringUpdate).mockReset().mockResolvedValue(rent)
  vi.mocked(api.recurringDelete).mockReset().mockResolvedValue(undefined)
  vi.mocked(api.recurringPost).mockReset().mockResolvedValue({
    entry: {
      id: 'j1',
      entity_id: 'e1',
      entry_date: '2026-08-01',
      description: 'Rent',
      reference: null,
      status: 'posted',
      hidden: false,
    },
    lines: [],
    is_voided: false,
  })
  vi.mocked(api.accountList).mockReset().mockResolvedValue(accounts)
  vi.mocked(api.accountDefaults).mockReset().mockResolvedValue(DEFAULTS)
})

async function renderPage(onBack = vi.fn()) {
  render(<RecurringPage entity={entity} onBack={onBack} />)
  await waitFor(() => {
    expect(api.recurringList).toHaveBeenCalledWith('e1')
  })
  return onBack
}

describe('RecurringPage empty state', () => {
  test('renders the page title, empty copy, local-only meta, and New template', async () => {
    await renderPage()
    expect(screen.getByRole('heading', { level: 1, name: 'Recurring' })).toBeTruthy()
    expect(screen.getAllByRole('heading', { name: 'Recurring' }).length).toBeGreaterThan(0)
    expect(screen.getByRole('heading', { name: 'Templates' })).toBeTruthy()
    expect(screen.getByText('A lightweight template — not a second ledger.')).toBeTruthy()
    expect(screen.getByText('No recurring templates yet')).toBeTruthy()
    expect(
      screen.getByText(
        'Save rent, payroll, utilities, or a subscription as a template. You post each occurrence when it is due.',
      ),
    ).toBeTruthy()
    expect(screen.getByText('0 templates · local only')).toBeTruthy()
    expect(screen.getAllByRole('button', { name: 'New template' }).length).toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: 'Back to entries' })).toBeTruthy()
  })

  test('Back to entries calls onBack', async () => {
    const onBack = await renderPage()
    await userEvent.click(screen.getByRole('button', { name: 'Back to entries' }))
    expect(onBack).toHaveBeenCalledTimes(1)
  })
})

describe('RecurringPage list affordances', () => {
  beforeEach(() => {
    vi.mocked(api.recurringList).mockResolvedValue([rent, payroll])
  })

  test('shows Due pill and solid Post when due; ghost Post when not', async () => {
    await renderPage()
    expect(screen.getByRole('heading', { name: 'Templates' })).toBeTruthy()
    expect(screen.getByText('2 templates · 1 due · EUR')).toBeTruthy()
    expect(screen.getByText('Due')).toBeTruthy()
    expect(screen.getByText('Expense')).toBeTruthy()
    expect(screen.getByText('Income')).toBeTruthy()
    expect(screen.getByText('local only')).toBeTruthy()
    expect(screen.getByText(/Housing → Checking/)).toBeTruthy()
    expect(screen.getByText(/Salary → Checking/)).toBeTruthy()

    const postButtons = screen.getAllByRole('button', { name: 'Post' })
    expect(postButtons).toHaveLength(2)
    expect(postButtons[0].getAttribute('data-variant')).toBe('primary')
    expect(postButtons[1].getAttribute('data-variant')).toBe('ghost')
  })

  test('income amount uses the Ledger in tint and a + prefix', async () => {
    await renderPage()
    const payrollAmount = screen.getByText(/^\+/)
    expect(payrollAmount.className).toMatch(/--color-money-in-text/)
    expect(payrollAmount.textContent).toMatch(/\+/)
  })
})

describe('RecurringPage new template modal', () => {
  test('fields follow Writer labels and save invokes recurringCreate', async () => {
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New template' })).toBeTruthy()
    })
    expect(screen.getAllByText('A lightweight template — not a second ledger.').length).toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: 'Expense' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Income' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Bill' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Transfer' })).toBeTruthy()
    for (const kind of ['Expense', 'Income', 'Bill', 'Transfer'] as const) {
      expect(screen.getByRole('button', { name: kind }).querySelector('svg')).toBeTruthy()
    }
    expect(screen.getByPlaceholderText('e.g. Rent')).toBeTruthy()
    expect(screen.getByText('Amount (EUR)')).toBeTruthy()
    expect(screen.getByLabelText('Cadence')).toBeTruthy()
    expect(screen.getByLabelText('Day of month')).toBeTruthy()
    expect(screen.getByText('Category')).toBeTruthy()
    expect(screen.getByText('From account')).toBeTruthy()
    expect(screen.getByPlaceholderText('Optional note on each posted entry')).toBeTruthy()

    const cadence = screen.getByLabelText('Cadence')
    expect(within(cadence).getByRole('option', { name: 'Monthly' })).toBeTruthy()
    expect(within(cadence).getByRole('option', { name: 'Weekly' })).toBeTruthy()
    expect(within(cadence).getByRole('option', { name: 'Yearly' })).toBeTruthy()

    await userEvent.type(screen.getByPlaceholderText('e.g. Rent'), 'Rent')
    await userEvent.type(screen.getByRole('textbox', { name: /amount/i }), '850')
    await userEvent.click(screen.getByRole('button', { name: 'Save template' }))

    await waitFor(() => {
      expect(api.recurringCreate).toHaveBeenCalledTimes(1)
    })
    expect(api.recurringCreate).toHaveBeenCalledWith({
      entity_id: 'e1',
      name: 'Rent',
      kind: 'expense',
      bill_status: null,
      amount_minor: 85000,
      cadence: 'monthly',
      day_of_month: 1,
      memo: null,
      next_date: expect.any(String),
      category_account_id: 'exp1',
      wallet_account_id: 'w1',
      payable_account_id: null,
      from_account_id: null,
      to_account_id: null,
    })
  })

  test('the day-of-month hint sits outside the field box, not inside it', async () => {
    // A Field renders as one glass field box containing only its label and
    // control; a helper paragraph inside it would sit inside that box too.
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await waitFor(() => {
      expect(screen.getByLabelText('Day of month')).toBeTruthy()
    })

    const hint = screen.getByText('Used when Cadence is Monthly.')
    const fieldBox = screen.getByLabelText('Day of month').closest('.field-box')
    expect(fieldBox).not.toBeNull()
    expect(fieldBox?.contains(hint)).toBe(false)
  })

  test('Weekly hides day of month and sends null', async () => {
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await waitFor(() => {
      expect(screen.getByLabelText('Cadence')).toBeTruthy()
    })
    await userEvent.selectOptions(screen.getByLabelText('Cadence'), 'weekly')
    expect(screen.queryByLabelText('Day of month')).toBeNull()

    await userEvent.type(screen.getByPlaceholderText('e.g. Rent'), 'Groceries')
    await userEvent.type(screen.getByRole('textbox', { name: /amount/i }), '40')
    await userEvent.click(screen.getByRole('button', { name: 'Save template' }))

    await waitFor(() => {
      expect(api.recurringCreate).toHaveBeenCalledTimes(1)
    })
    expect(api.recurringCreate.mock.calls[0]?.[0]).toMatchObject({
      cadence: 'weekly',
      day_of_month: null,
    })
  })
})

describe('RecurringPage default accounts', () => {
  test('the form preselects the accounts account_defaults returned', async () => {
    // Neither default is the first account of its type: only Rust's answer
    // can pick them.
    vi.mocked(api.accountList).mockResolvedValue([
      ...accounts,
      account({ id: 'exp2', name: 'Λογαριασμός 5300', account_type: 'expense', code: '5300' }),
      account({ id: 'w2', name: 'Λογαριασμός 1010', account_type: 'asset', code: '1010' }),
    ])
    vi.mocked(api.accountDefaults).mockResolvedValue({
      ...DEFAULTS,
      bill_category: 'exp2',
      payment: 'w2',
      transfer_source: 'w2',
      transfer_destination: 'w1',
    })

    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])

    await waitFor(() => {
      expect(screen.getByLabelText('Category')).toHaveValue('exp2')
    })
    expect(screen.getByLabelText('From account')).toHaveValue('w2')
    expect(api.accountDefaults).toHaveBeenCalledWith('e1')

    await userEvent.click(screen.getByRole('button', { name: 'Transfer' }))

    await waitFor(() => {
      expect(screen.getByLabelText('To')).toHaveValue('w1')
    })
    expect(screen.getByLabelText('From account')).toHaveValue('w2')
  })
})

describe('RecurringPage post confirm', () => {
  beforeEach(() => {
    vi.mocked(api.recurringList).mockResolvedValue([rent])
  })

  test('Post opens confirm and recurring_post sends id plus date/amount overrides', async () => {
    await renderPage()
    await userEvent.click(screen.getByRole('button', { name: 'Post' }))
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Post Rent?' })).toBeTruthy()
    })
    expect(
      screen.getByText(
        'This writes one journal entry from the template. Nothing else is scheduled.',
      ),
    ).toBeTruthy()
    expect(within(screen.getByRole('dialog', { name: 'Post Rent?' })).getByText('Expense')).toBeTruthy()
    expect(within(screen.getByRole('dialog', { name: 'Post Rent?' })).getByText(/Housing/)).toBeTruthy()

    const dialog = screen.getByRole('dialog', { name: 'Post Rent?' })
    const amount = within(dialog).getByLabelText('Amount (EUR)')
    await userEvent.clear(amount)
    await userEvent.type(amount, '900')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Post' }))

    await waitFor(() => {
      expect(api.recurringPost).toHaveBeenCalledTimes(1)
    })
    expect(api.recurringPost).toHaveBeenCalledWith('r1', {
      entry_date: '2026-08-01',
      amount_minor: 90000,
    })
  })
})

describe('RecurringPage i18n', () => {
  test('EL empty chrome uses Writer keys', async () => {
    setLocale('el')
    await renderPage()
    expect(screen.getByRole('heading', { level: 1, name: 'Επαναλαμβανόμενα' })).toBeTruthy()
    expect(screen.getByText('Δεν υπάρχουν επαναλαμβανόμενα πρότυπα ακόμη')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Επιστροφή στις καταχωρίσεις' })).toBeTruthy()
    expect(screen.getAllByRole('button', { name: 'Νέο πρότυπο' }).length).toBeGreaterThan(0)
  })
})

describe('RecurringPage template type colours', () => {
  test('the chosen type wears its Ledger gradient; Transfer stays neutral', async () => {
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await screen.findByRole('heading', { name: 'New template' })

    expect(screen.getByRole('button', { name: 'Expense' })).toHaveAttribute('data-tone', 'money-out')

    await userEvent.click(screen.getByRole('button', { name: 'Income' }))
    expect(screen.getByRole('button', { name: 'Income' })).toHaveAttribute('data-tone', 'money-in')

    await userEvent.click(screen.getByRole('button', { name: 'Transfer' }))
    expect(screen.getByRole('button', { name: 'Transfer' })).toHaveAttribute('data-tone', 'neutral')
  })
})
