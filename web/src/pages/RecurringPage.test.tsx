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
  base_currency_decimals: 2,
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
  receivable: null,
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
      listed_kind: 'expense',
      simple_form: null,
      settles_entry_id: null,
      settled_by_entry_id: null,
    },
    template: rent,
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
    expect(vi.mocked(api.recurringCreate).mock.calls[0]?.[0]).toMatchObject({
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

describe('RecurringPage post dialog keyboard', () => {
  test('Enter in the amount field posts the template', async () => {
    vi.mocked(api.recurringList).mockResolvedValue([rent])
    await renderPage()
    await userEvent.click(screen.getByRole('button', { name: 'Post' }))
    const dialog = await screen.findByRole('dialog', { name: 'Post Rent?' })

    await userEvent.type(within(dialog).getByLabelText('Amount (EUR)'), '{Enter}')

    await waitFor(() => {
      expect(api.recurringPost).toHaveBeenCalledTimes(1)
    })
  })
})

describe('RecurringPage weekly and yearly start dates', () => {
  beforeEach(() => {
    // Thursday 8 October 2026. Only Date is faked, so user-event keeps its timers.
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime(new Date(2026, 9, 8, 12, 0, 0))
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  async function openNewTemplate() {
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await userEvent.type(await screen.findByPlaceholderText('e.g. Rent'), 'Cleaner')
    await userEvent.type(screen.getByRole('textbox', { name: /amount/i }), '40')
  }

  test('Weekly offers a weekday and starts on the next such day', async () => {
    await openNewTemplate()
    await userEvent.selectOptions(screen.getByLabelText('Cadence'), 'weekly')

    await userEvent.selectOptions(screen.getByLabelText('Day of the week'), 'Friday')

    expect(screen.getByText('Next date: 09/10/2026')).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Save template' }))
    await waitFor(() => {
      expect(api.recurringCreate).toHaveBeenCalledTimes(1)
    })
    expect(vi.mocked(api.recurringCreate).mock.calls[0]?.[0]).toMatchObject({
      cadence: 'weekly',
      day_of_month: null,
      next_date: '2026-10-09',
    })
  })

  test('Yearly offers a month and day and starts on the next such date', async () => {
    await openNewTemplate()
    await userEvent.selectOptions(screen.getByLabelText('Cadence'), 'yearly')

    await userEvent.selectOptions(screen.getByLabelText('Month'), 'March')
    await userEvent.selectOptions(screen.getByLabelText('Day of month'), '15')

    expect(screen.getByText('Next date: 15/03/2027')).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Save template' }))
    await waitFor(() => {
      expect(api.recurringCreate).toHaveBeenCalledTimes(1)
    })
    expect(vi.mocked(api.recurringCreate).mock.calls[0]?.[0]).toMatchObject({
      cadence: 'yearly',
      day_of_month: null,
      next_date: '2027-03-15',
    })
  })

  test('Yearly pulls the day back when the month is shorter', async () => {
    await openNewTemplate()
    await userEvent.selectOptions(screen.getByLabelText('Cadence'), 'yearly')
    await userEvent.selectOptions(screen.getByLabelText('Day of month'), '31')

    await userEvent.selectOptions(screen.getByLabelText('Month'), 'April')

    expect(screen.getByLabelText('Day of month')).toHaveValue('30')
    expect(screen.getByText('Next date: 30/04/2027')).toBeTruthy()
  })

  test('editing a weekly template shows its date and weekday without moving them', async () => {
    vi.mocked(api.recurringList).mockResolvedValue([
      template({ cadence: 'weekly', day_of_month: null, next_date: '2026-10-14', due: false }),
    ])
    await renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Edit template' }))

    expect(await screen.findByLabelText('Day of the week')).toHaveValue('3')
    expect(screen.getByText('Next date: 14/10/2026')).toBeTruthy()
  })
})

describe('RecurringPage bill and income status', () => {
  const withPayables: Account[] = [
    ...accounts,
    account({ id: 'bp1', name: 'Bills payable', account_type: 'liability', code: '2050' }),
    account({ id: 'rcv1', name: 'Receivables', account_type: 'asset', code: '1200' }),
  ]

  beforeEach(() => {
    vi.mocked(api.accountList).mockResolvedValue(withPayables)
    vi.mocked(api.accountDefaults).mockResolvedValue({
      ...DEFAULTS,
      bills_payable: 'bp1',
      receivable: 'rcv1',
    })
  })

  async function openNewBill() {
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await userEvent.click(await screen.findByRole('button', { name: 'Bill' }))
    await userEvent.type(screen.getByPlaceholderText('e.g. Rent'), 'Power')
    await userEvent.type(screen.getByRole('textbox', { name: /amount/i }), '40')
  }

  test('a new bill offers three statuses and saves an unpaid one with its payable', async () => {
    await openNewBill()
    const status = screen.getByLabelText('Bill status')
    expect(within(status).getAllByRole('option').map((option) => option.textContent)).toEqual([
      'Paid now (from bank/card)',
      'Unpaid — I owe this (bills payable)',
      'Pay an existing unpaid bill',
    ])

    await userEvent.selectOptions(status, 'unpaid')
    expect(screen.getByLabelText('Bills payable account')).toHaveValue('bp1')
    expect(screen.queryByLabelText('Paid from')).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Save template' }))

    await waitFor(() => {
      expect(api.recurringCreate).toHaveBeenCalledTimes(1)
    })
    expect(vi.mocked(api.recurringCreate).mock.calls[0]?.[0]).toMatchObject({
      kind: 'bill',
      bill_status: 'unpaid',
      category_account_id: 'exp1',
      payable_account_id: 'bp1',
    })
  })

  test('a payment of an existing bill says which bill each post settles', async () => {
    await openNewBill()
    await userEvent.selectOptions(screen.getByLabelText('Bill status'), 'pay_existing')

    expect(
      screen.getByText(/settles the oldest open bill on the bills payable account/),
    ).toBeInTheDocument()
    expect(screen.queryByLabelText('Bill category')).toBeNull()
    expect(screen.getByLabelText('Pay from')).toHaveValue('w1')
    await userEvent.click(screen.getByRole('button', { name: 'Save template' }))

    await waitFor(() => {
      expect(api.recurringCreate).toHaveBeenCalledTimes(1)
    })
    expect(vi.mocked(api.recurringCreate).mock.calls[0]?.[0]).toMatchObject({
      kind: 'bill',
      bill_status: 'pay_existing',
      wallet_account_id: 'w1',
      payable_account_id: 'bp1',
    })
  })

  test('editing an unpaid bill opens on its stored status and payable', async () => {
    const owed = template({
      id: 'r3',
      name: 'Power',
      kind: 'bill',
      bill_status: 'unpaid',
      category_account_id: 'exp1',
      wallet_account_id: null,
      payable_account_id: 'bp1',
    })
    vi.mocked(api.recurringList).mockResolvedValue([owed])
    await renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Edit template' }))

    expect(screen.getByLabelText('Bill status')).toHaveValue('unpaid')
    expect(screen.getByLabelText('Bills payable account')).toHaveValue('bp1')
    await userEvent.click(screen.getByRole('button', { name: 'Save template' }))

    await waitFor(() => {
      expect(api.recurringUpdate).toHaveBeenCalledTimes(1)
    })
    expect(vi.mocked(api.recurringUpdate).mock.calls[0]?.[0]).toMatchObject({
      id: 'r3',
      kind: 'bill',
      bill_status: 'unpaid',
      category_account_id: 'exp1',
      payable_account_id: 'bp1',
    })
  })

  test('editing an income still owed opens on its receivable and keeps it owed', async () => {
    const invoiced = template({
      id: 'r4',
      name: 'Consulting',
      kind: 'income',
      bill_status: 'unpaid',
      category_account_id: 'inc1',
      wallet_account_id: null,
      payable_account_id: 'rcv1',
    })
    vi.mocked(api.recurringList).mockResolvedValue([invoiced])
    await renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Edit template' }))

    expect(screen.getByLabelText('Income status')).toHaveValue('unpaid')
    expect(screen.getByLabelText('Receivable account')).toHaveValue('rcv1')
    await userEvent.click(screen.getByRole('button', { name: 'Save template' }))

    await waitFor(() => {
      expect(api.recurringUpdate).toHaveBeenCalledTimes(1)
    })
    expect(vi.mocked(api.recurringUpdate).mock.calls[0]?.[0]).toMatchObject({
      kind: 'income',
      bill_status: 'unpaid',
      category_account_id: 'inc1',
      payable_account_id: 'rcv1',
    })
  })

  test('posting a payment of an existing bill says which bill it settles', async () => {
    const payment = template({
      id: 'r5',
      name: 'Power payment',
      kind: 'bill',
      bill_status: 'pay_existing',
      category_account_id: null,
      wallet_account_id: 'w1',
      payable_account_id: 'bp1',
    })
    vi.mocked(api.recurringList).mockResolvedValue([payment])
    await renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Post' }))

    const dialog = await screen.findByRole('dialog')
    expect(
      within(dialog).getByText(/settles the oldest open bill on the bills payable account/),
    ).toBeInTheDocument()
    expect(within(dialog).getByText('Bills payable → Checking')).toBeInTheDocument()
  })
})

describe('RecurringPage status fields keep and reset what they hold', () => {
  const withPayables: Account[] = [
    ...accounts,
    account({ id: 'bp1', name: 'Bills payable', account_type: 'liability', code: '2050' }),
    account({ id: 'rcv1', name: 'Receivables', account_type: 'asset', code: '1200' }),
  ]

  beforeEach(() => {
    vi.mocked(api.accountList).mockResolvedValue(withPayables)
    vi.mocked(api.accountDefaults).mockResolvedValue({
      ...DEFAULTS,
      bills_payable: 'bp1',
      receivable: 'rcv1',
    })
  })

  const payment = template({
    id: 'r6',
    name: 'Power payment',
    kind: 'bill',
    bill_status: 'pay_existing',
    category_account_id: null,
    wallet_account_id: 'w1',
    payable_account_id: 'bp1',
  })

  test('editing a payment of an existing bill opens on its status and accounts', async () => {
    vi.mocked(api.recurringList).mockResolvedValue([payment])
    await renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Edit template' }))

    const status = screen.getByLabelText('Bill status')
    expect(status).toHaveValue('pay_existing')
    expect(status).toHaveAccessibleDescription(/settles the oldest open bill/)
    expect(screen.queryByLabelText('Bill category')).toBeNull()
    expect(screen.getByLabelText('Pay from')).toHaveValue('w1')
    expect(screen.getByLabelText('Bills payable account')).toHaveValue('bp1')
    await userEvent.click(screen.getByRole('button', { name: 'Save template' }))

    await waitFor(() => {
      expect(api.recurringUpdate).toHaveBeenCalledTimes(1)
    })
    expect(vi.mocked(api.recurringUpdate).mock.calls[0]?.[0]).toMatchObject({
      kind: 'bill',
      bill_status: 'pay_existing',
      wallet_account_id: 'w1',
      payable_account_id: 'bp1',
    })
  })

  test('an owed income saves its receivable; received lists assets only', async () => {
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await userEvent.click(await screen.findByRole('button', { name: 'Income' }))

    const receivedInto = screen.getByLabelText('Received into')
    const offered = within(receivedInto)
      .getAllByRole('option')
      .map((option) => option.getAttribute('value'))
    expect(offered).toEqual(['w1', 'rcv1'])

    await userEvent.selectOptions(screen.getByLabelText('Income status'), 'unpaid')
    expect(screen.getByLabelText('Receivable account')).toHaveValue('rcv1')
    await userEvent.type(screen.getByPlaceholderText('e.g. Rent'), 'Consulting')
    await userEvent.type(screen.getByRole('textbox', { name: /amount/i }), '900')
    await userEvent.click(screen.getByRole('button', { name: 'Save template' }))

    await waitFor(() => {
      expect(api.recurringCreate).toHaveBeenCalledTimes(1)
    })
    expect(vi.mocked(api.recurringCreate).mock.calls[0]?.[0]).toMatchObject({
      kind: 'income',
      bill_status: 'unpaid',
      category_account_id: 'inc1',
      payable_account_id: 'rcv1',
    })
  })

  test('another kind starts again from its defaults, and so does New after an edit', async () => {
    vi.mocked(api.recurringList).mockResolvedValue([payment])
    await renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Edit template' }))
    expect(screen.getByLabelText('Bill status')).toHaveValue('pay_existing')

    await userEvent.click(screen.getByRole('button', { name: 'Income' }))
    expect(screen.getByLabelText('Income status')).toHaveValue('received')
    await userEvent.click(screen.getByRole('button', { name: 'Bill' }))
    expect(screen.getByLabelText('Bill status')).toHaveValue('paid')

    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await userEvent.click(await screen.findByRole('button', { name: 'Bill' }))
    expect(screen.getByLabelText('Bill status')).toHaveValue('paid')
    await userEvent.selectOptions(screen.getByLabelText('Bill status'), 'unpaid')
    expect(screen.getByLabelText('Bills payable account')).toHaveValue('bp1')
  })

  test('a book with no liability says how to add a bills payable account', async () => {
    vi.mocked(api.accountList).mockResolvedValue(accounts)
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await userEvent.click(await screen.findByRole('button', { name: 'Bill' }))
    await userEvent.selectOptions(screen.getByLabelText('Bill status'), 'unpaid')

    const payable = screen.getByLabelText('Bills payable account')
    expect(within(payable).getByRole('option', { name: 'No liability accounts — add one' }))
      .toBeInTheDocument()
    expect(screen.getByText(/add a liability named “Bills Payable”/)).toBeInTheDocument()
  })
})

describe('RecurringPage account selects show what the form holds', () => {
  test('a template on an archived account shows it as archived and holds Save', async () => {
    vi.mocked(api.accountList).mockResolvedValue([
      ...accounts,
      account({
        id: 'exp0',
        name: 'Old rent',
        account_type: 'expense',
        code: '5900',
        is_active: false,
      }),
    ])
    vi.mocked(api.recurringList).mockResolvedValue([template({ category_account_id: 'exp0' })])
    await renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Edit template' }))

    const category = screen.getByLabelText('Category')
    expect(category).toHaveValue('exp0')
    expect(category).toHaveDisplayValue('5900 · Old rent (archived)')
    expect(category).toHaveAttribute('aria-invalid', 'true')
    const save = screen.getByRole('button', { name: 'Save template' })
    expect(save).toBeDisabled()
    expect(save).toHaveAccessibleDescription('Choose an account for every field this entry needs.')

    await userEvent.selectOptions(category, 'exp1')

    expect(save).toBeEnabled()
    await userEvent.click(save)
    await waitFor(() => {
      expect(api.recurringUpdate).toHaveBeenCalledTimes(1)
    })
    expect(vi.mocked(api.recurringUpdate).mock.calls[0]?.[0]).toMatchObject({
      category_account_id: 'exp1',
    })
  })

  test('a role with no default shows a placeholder, not the first account', async () => {
    vi.mocked(api.accountList).mockResolvedValue([
      ...accounts,
      account({ id: 'bp1', name: 'Bills payable', account_type: 'liability', code: '2050' }),
    ])
    // DEFAULTS names no bills payable account.
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])
    await userEvent.click(await screen.findByRole('button', { name: 'Bill' }))
    await userEvent.selectOptions(screen.getByLabelText('Bill status'), 'unpaid')

    const payable = screen.getByLabelText('Bills payable account')
    expect(payable).toHaveValue('')
    expect(payable).toHaveDisplayValue('Choose an account…')
    expect(screen.getByRole('button', { name: 'Save template' })).toBeDisabled()

    await userEvent.selectOptions(payable, 'bp1')

    expect(screen.getByRole('button', { name: 'Save template' })).toBeEnabled()
  })

  test('defaults that arrive after New opens fill its empty fields', async () => {
    let resolveDefaults: (roles: AccountDefaults) => void = () => {}
    vi.mocked(api.accountDefaults).mockReturnValue(
      new Promise((resolve) => {
        resolveDefaults = resolve
      }),
    )
    await renderPage()
    await userEvent.click(screen.getAllByRole('button', { name: 'New template' })[0])

    expect(screen.getByLabelText('Category')).toHaveDisplayValue('Choose an account…')
    expect(screen.getByRole('button', { name: 'Save template' })).toBeDisabled()

    resolveDefaults(DEFAULTS)

    await waitFor(() => {
      expect(screen.getByLabelText('Category')).toHaveValue('exp1')
    })
    expect(screen.getByLabelText('From account')).toHaveValue('w1')
    expect(screen.getByRole('button', { name: 'Save template' })).toBeEnabled()
  })
})
