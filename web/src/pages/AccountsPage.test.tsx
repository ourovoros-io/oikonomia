/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      accountList: vi.fn(),
      accountRegister: vi.fn(),
      accountBalance: vi.fn(),
      accountArchive: vi.fn(),
      accountUpdate: vi.fn(),
      accountCreate: vi.fn(),
      accountSetOpeningBalance: vi.fn(),
    },
  }
})

import type { Account, Entity, RegisterLine } from '../lib/api'
import { api } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { formatMoney } from '../lib/money'
import { AccountsPage } from './AccountsPage'

beforeEach(() => {
  vi.mocked(api.accountBalance).mockReset().mockResolvedValue(0)
})

afterEach(() => {
  cleanup()
  resetI18nForTests()
  vi.restoreAllMocks()
})

describe('AccountsPage empty-state CTA', () => {
  test('no-book empty state renders Create a book and calls onCreateBook', async () => {
    const onCreateBook = vi.fn()
    render(<AccountsPage entity={null} onCreateBook={onCreateBook} />)
    const cta = screen.getByRole('button', { name: 'Create a book' })
    await userEvent.click(cta)
    expect(onCreateBook).toHaveBeenCalledTimes(1)
  })

  test('renders without a CTA when onCreateBook is not supplied', () => {
    render(<AccountsPage entity={null} />)
    expect(screen.queryByRole('button', { name: 'Create a book' })).toBeNull()
  })
})

describe('AccountsPage account type colours', () => {
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

  function badgeClass(name: string): string {
    return screen.getByText(name).closest('li')?.firstElementChild?.className ?? ''
  }

  test('income and expense wear the Ledger money tones, never status colours', async () => {
    vi.mocked(api.accountList).mockResolvedValue([
      account({ id: 'inc', name: 'Salary', account_type: 'income' }),
      account({ id: 'exp', name: 'Groceries', account_type: 'expense' }),
    ])
    render(<AccountsPage entity={entity} />)
    await waitFor(() => expect(screen.getByText('Salary')).toBeTruthy())

    expect(badgeClass('Salary')).toContain('bg-[var(--color-money-in-soft)]')
    expect(badgeClass('Groceries')).toContain('bg-[var(--color-money-out-soft)]')
  })

  test('asset, liability and equity never wear success, danger or warning, and stay distinguishable', async () => {
    vi.mocked(api.accountList).mockResolvedValue([
      account({ id: 'ast', name: 'Checking', account_type: 'asset' }),
      account({ id: 'lia', name: 'Credit Card', account_type: 'liability' }),
      account({ id: 'eq', name: 'Owner Equity', account_type: 'equity' }),
    ])
    render(<AccountsPage entity={entity} />)
    await waitFor(() => expect(screen.getByText('Checking')).toBeTruthy())

    const classes = ['Checking', 'Credit Card', 'Owner Equity'].map(badgeClass)
    for (const cls of classes) {
      expect(cls).not.toMatch(/--color-success|--color-danger|--color-warning/)
    }
    expect(new Set(classes).size).toBe(3)
  })
})

describe('AccountsPage register drill-in', () => {
  const entity: Entity = {
    id: 'e1',
    name: 'Personal',
    base_currency: 'EUR',
    base_currency_decimals: 2,
    fiscal_year_start_month: 1,
    chart_template: 'personal',
  }

  const checking: Account = {
    id: 'w1',
    entity_id: 'e1',
    code: '1000',
    name: 'Checking',
    account_type: 'asset',
    is_active: true,
    is_system: false,
    sort_order: 0,
  }

  const lines: RegisterLine[] = [
    {
      entry_id: 'j1',
      entry_date: '2026-08-11',
      description: 'CLIENT INVOICE',
      debit_minor: 120000,
      credit_minor: 0,
      balance_minor: 125000,
      hidden: false,
    },
    {
      entry_id: 'j2',
      entry_date: '2026-08-12',
      description: 'ALPHA SUPERMARKET',
      debit_minor: 0,
      credit_minor: 4250,
      balance_minor: 120750,
      hidden: true,
    },
  ]

  async function renderReady(accounts: Account[] = [checking]) {
    vi.mocked(api.accountList).mockResolvedValue(accounts)
    render(<AccountsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByText('Checking')).toBeTruthy()
    })
  }

  /**
   * formatMoney renders a NBSP before the currency symbol; RTL's default
   * text normalizer collapses the DOM's NBSP to a plain space before
   * matching but does not touch the matcher string, so callers must
   * normalize the same way or an exact-string match never hits.
   */
  function money(minor: number): string {
    return formatMoney(minor, { code: 'EUR', decimals: 2 }).replace(/ /g, ' ')
  }

  test('row action shows the register table with running balance', async () => {
    vi.mocked(api.accountRegister).mockResolvedValue(lines)
    await renderReady()

    await userEvent.click(screen.getByRole('button', { name: /checking register/i }))

    expect(api.accountRegister).toHaveBeenCalledWith('w1')
    await waitFor(() => {
      expect(screen.getByText('CLIENT INVOICE')).toBeTruthy()
    })
    const incomeRow = screen.getByText('CLIENT INVOICE').closest('tr') as HTMLElement
    expect(within(incomeRow).getByText(money(120000))).toBeTruthy()
    expect(within(incomeRow).getByText(money(125000))).toBeTruthy()

    const expenseRow = screen.getByText('ALPHA SUPERMARKET').closest('tr') as HTMLElement
    expect(within(expenseRow).getByText(money(4250))).toBeTruthy()
    expect(within(expenseRow).getByText(money(120750))).toBeTruthy()
  })

  test('hidden line is visually muted', async () => {
    vi.mocked(api.accountRegister).mockResolvedValue(lines)
    await renderReady()

    await userEvent.click(screen.getByRole('button', { name: /checking register/i }))

    const hiddenRow = await waitFor(() => {
      const row = screen.getByText('ALPHA SUPERMARKET').closest('tr')
      expect(row).toBeTruthy()
      return row as HTMLElement
    })
    expect(hiddenRow.className).toMatch(/opacity/)
    expect(screen.getByText('Hidden')).toBeTruthy()
  })

  test('register load failure shows the localized copy for its code in the ErrorBanner', async () => {
    vi.mocked(api.accountRegister).mockRejectedValue({
      code: 'not_found',
      message: 'account 42 not found',
    })
    await renderReady()

    await userEvent.click(screen.getByRole('button', { name: /checking register/i }))

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('That item could not be found.')
    })
    expect(screen.queryByText(/account 42/)).toBeNull()
  })

  test('register load failure with an unknown code never shows the raw message', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.accountRegister).mockRejectedValue({
      code: 'register_load_boom',
      message: 'could not load register',
    })
    await renderReady()

    await userEvent.click(screen.getByRole('button', { name: /checking register/i }))

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Something went wrong.')
    })
    expect(screen.queryByText('could not load register')).toBeNull()
  })

  test('empty register shows accounts.register.empty copy', async () => {
    vi.mocked(api.accountRegister).mockResolvedValue([])
    await renderReady()

    await userEvent.click(screen.getByRole('button', { name: /checking register/i }))

    await waitFor(() => {
      expect(screen.getByText('No posted entries for this account yet.')).toBeTruthy()
    })
  })

  test('back button restores the account list', async () => {
    vi.mocked(api.accountRegister).mockResolvedValue(lines)
    await renderReady()

    await userEvent.click(screen.getByRole('button', { name: /checking register/i }))
    await waitFor(() => {
      expect(screen.getByText('CLIENT INVOICE')).toBeTruthy()
    })

    await userEvent.click(screen.getByRole('button', { name: 'All accounts' }))

    expect(screen.queryByText('CLIENT INVOICE')).toBeNull()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /checking register/i })).toBeTruthy()
    })
  })
})

describe('AccountsPage chart', () => {
  const entity: Entity = {
    id: 'e1',
    name: 'Personal',
    base_currency: 'EUR',
    base_currency_decimals: 2,
    fiscal_year_start_month: 1,
    chart_template: 'personal',
  }

  function account(over: Partial<Account> & Pick<Account, 'id' | 'name' | 'account_type' | 'code'>): Account {
    return { entity_id: 'e1', is_active: true, is_system: false, sort_order: 0, ...over }
  }

  const checking = account({ id: 'w1', code: '1010', name: 'Checking', account_type: 'asset' })
  const card = account({ id: 'c1', code: '2000', name: 'Credit Card', account_type: 'liability' })
  const coffee = account({ id: 'x1', code: '5150', name: 'Coffee', account_type: 'expense' })
  const meals = account({ id: 'x2', code: '5100', name: 'Meals', account_type: 'expense' })

  const money = (minor: number) =>
    formatMoney(minor, { code: 'EUR', decimals: 2 }).replace(/\u00a0/g, ' ')

  function rowOf(name: string): HTMLElement {
    return screen.getByText(name).closest('li') as HTMLElement
  }

  async function renderChart(accounts: Account[]) {
    vi.mocked(api.accountList).mockResolvedValue(accounts)
    render(<AccountsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByText(accounts[0].name)).toBeTruthy()
    })
  }

  test('rows are in code order and balance-sheet rows show their balance', async () => {
    vi.mocked(api.accountBalance).mockImplementation(async (id) => (id === 'c1' ? 35000 : 250000))
    await renderChart([coffee, card, meals, checking])

    const codes = screen.getAllByRole('listitem').map((row) => row.textContent?.slice(0, 4))
    expect(codes).toEqual(['1010', '2000', '5100', '5150'])

    // An amount owed is shown as a positive balance on the card.
    await waitFor(() => {
      expect(within(rowOf('Credit Card')).getByText(money(35000))).toBeTruthy()
    })
    expect(within(rowOf('Checking')).getByText(money(250000))).toBeTruthy()
    expect(api.accountBalance).not.toHaveBeenCalledWith('x1', expect.any(String))
  })

  test('clicking a row opens its register', async () => {
    vi.mocked(api.accountRegister).mockResolvedValue([])
    await renderChart([checking])

    await userEvent.click(screen.getByText('Checking'))

    expect(api.accountRegister).toHaveBeenCalledWith('w1')
  })

  test('deactivating asks first, and an inactive account can be reactivated', async () => {
    vi.mocked(api.accountArchive).mockResolvedValue(undefined)
    await renderChart([coffee])

    await userEvent.click(within(rowOf('Coffee')).getByRole('button', { name: 'Deactivate account' }))

    expect(api.accountArchive).not.toHaveBeenCalled()
    const dialog = screen.getByRole('dialog', { name: 'Deactivate Coffee?' })
    await userEvent.click(within(dialog).getByRole('button', { name: 'Deactivate' }))
    await waitFor(() => {
      expect(api.accountArchive).toHaveBeenCalledWith('x1')
    })
    expect(await screen.findByText('Coffee deactivated.')).toBeTruthy()
  })

  test('Reactivate puts an inactive account back with its own code, name and order', async () => {
    const inactive = { ...coffee, is_active: false, sort_order: 7 }
    vi.mocked(api.accountUpdate).mockResolvedValue(coffee)
    await renderChart([inactive])

    await userEvent.click(screen.getByRole('button', { name: 'Reactivate Coffee' }))

    await waitFor(() => {
      expect(api.accountUpdate).toHaveBeenCalledWith({
        id: 'x1',
        code: '5150',
        name: 'Coffee',
        is_active: true,
        sort_order: 7,
      })
    })
  })

  test('Rename changes the name and keeps the rest of the account', async () => {
    vi.mocked(api.accountUpdate).mockResolvedValue(coffee)
    await renderChart([coffee])

    await userEvent.click(screen.getByRole('button', { name: 'Rename Coffee' }))
    const name = within(screen.getByRole('dialog', { name: 'Rename account' })).getByRole('textbox')
    await userEvent.clear(name)
    await userEvent.type(name, 'Cafe')
    await userEvent.click(screen.getByRole('button', { name: 'Rename' }))

    await waitFor(() => {
      expect(api.accountUpdate).toHaveBeenCalledWith({
        id: 'x1',
        code: '5150',
        name: 'Cafe',
        is_active: true,
        sort_order: 0,
      })
    })
  })

  test('Add account stays open for the next account', async () => {
    vi.mocked(api.accountCreate).mockResolvedValue(meals)
    await renderChart([checking])

    await userEvent.click(screen.getByRole('button', { name: 'Add account' }))
    await userEvent.type(screen.getByLabelText('Code'), '5100')
    await userEvent.type(screen.getByLabelText('Name'), 'Meals')
    await userEvent.click(screen.getByRole('button', { name: 'Create account' }))

    await waitFor(() => {
      expect(api.accountCreate).toHaveBeenCalledTimes(1)
    })
    expect(await screen.findByText('Account 5100 · Meals added.')).toBeTruthy()
    expect(screen.getByLabelText('Code')).toHaveValue('')
    expect(screen.getByLabelText('Name')).toHaveValue('')
  })
})

describe('AccountsPage Set balance', () => {
  const entity: Entity = {
    id: 'e1',
    name: 'Personal',
    base_currency: 'EUR',
    base_currency_decimals: 2,
    fiscal_year_start_month: 1,
    chart_template: 'personal',
  }
  const base = { entity_id: 'e1', is_active: true, is_system: false, sort_order: 0 }
  const checking: Account = { ...base, id: 'w1', code: '1010', name: 'Checking', account_type: 'asset' }
  const card: Account = { ...base, id: 'c1', code: '2000', name: 'Credit Card', account_type: 'liability' }

  async function openSetBalance(account: Account) {
    vi.mocked(api.accountList).mockResolvedValue([checking, card])
    render(<AccountsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByText(account.name)).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: `Set balance for ${account.name}` }))
    return screen.getByRole('dialog')
  }

  test('a liability asks for the amount owed, explains the sign and warns on a negative', async () => {
    const dialog = await openSetBalance(card)

    expect(within(dialog).getByLabelText('Amount owed (EUR)')).toBeTruthy()
    expect(within(dialog).getByText(/Enter what you owe as a positive number/)).toBeTruthy()
    expect(within(dialog).queryByText(/in your favour, not a debt/)).toBeNull()

    await userEvent.type(within(dialog).getByLabelText('Amount owed (EUR)'), '-350')

    expect(within(dialog).getByText(/in your favour, not a debt/)).toBeTruthy()
  })

  test('an amount owed is sent as entered and confirmed after saving', async () => {
    vi.mocked(api.accountSetOpeningBalance).mockResolvedValue({} as never)
    const dialog = await openSetBalance(card)

    await userEvent.type(within(dialog).getByLabelText('Amount owed (EUR)'), '350')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Set balance' }))

    await waitFor(() => {
      expect(api.accountSetOpeningBalance).toHaveBeenCalledWith('c1', 35000, expect.any(String))
    })
    expect(await screen.findByText(/Balance of Credit Card set to/)).toBeTruthy()
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  test('an asset keeps the actual-balance wording and no debt hints', async () => {
    const dialog = await openSetBalance(checking)

    expect(within(dialog).getByLabelText('Actual balance (EUR)')).toBeTruthy()
    expect(within(dialog).queryByText(/Enter what you owe/)).toBeNull()
  })
})
