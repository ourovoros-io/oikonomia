/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type {
  Account,
  AccountDefaults,
  DocumentSuggestion,
  Entity,
  PostedEntryView,
  UiPrefs,
} from '../lib/api'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      entityList: vi.fn(),
      accountList: vi.fn(),
      accountDefaults: vi.fn(),
      getUiPrefs: vi.fn(),
      entryPostSimple: vi.fn(),
      documentAnalyze: vi.fn(),
      entryPostSimpleWithDocument: vi.fn(),
      entrySetHidden: vi.fn(),
      entryVoid: vi.fn(),
      rememberQuickAdd: vi.fn(),
    },
  }
})

import { api } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { QuickAddPage } from './QuickAddPage'

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  base_currency_decimals: 2,
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

const accounts: Account[] = [
  {
    id: 'w1',
    entity_id: 'e1',
    code: '1000',
    name: 'Checking',
    account_type: 'asset',
    is_active: true,
    is_system: false,
    sort_order: 0,
  },
  {
    id: 'exp1',
    entity_id: 'e1',
    code: '5000',
    name: 'Meals & dining',
    account_type: 'expense',
    is_active: true,
    is_system: false,
    sort_order: 1,
  },
]

const posted: PostedEntryView = {
  entry: {
    id: 'j1',
    entity_id: 'e1',
    entry_date: '2026-08-12',
    description: 'ATM cash',
    reference: null,
    status: 'posted',
    hidden: false,
  },
  lines: [],
  is_voided: false,
}

const prefs: UiPrefs = {
  last_entity_id: 'e1',
  last_accounts_by_entity_kind: {},
}

const DEFAULTS: AccountDefaults = {
  category: 'exp1',
  payment: 'w1',
  deposit: 'w1',
  income: null,
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
  vi.mocked(api.entityList).mockReset().mockResolvedValue([entity])
  vi.mocked(api.accountList).mockReset().mockResolvedValue(accounts)
  vi.mocked(api.accountDefaults).mockReset().mockResolvedValue(DEFAULTS)
  vi.mocked(api.getUiPrefs).mockReset().mockResolvedValue(prefs)
  vi.mocked(api.entryPostSimple).mockReset().mockResolvedValue(posted)
  vi.mocked(api.entrySetHidden).mockReset().mockResolvedValue({
    ...posted,
    entry: { ...posted.entry, hidden: true },
  })
  vi.mocked(api.rememberQuickAdd).mockReset().mockResolvedValue(undefined)
  vi.mocked(api.entryVoid).mockReset()
  vi.mocked(api.entryPostSimpleWithDocument).mockReset().mockResolvedValue(posted)
})

const ROLL_MS = 230

async function afterRoll() {
  await new Promise((resolve) => setTimeout(resolve, ROLL_MS + 40))
}

async function reachSaveStep(onPosted: () => void = () => {}) {
  render(<QuickAddPage onPosted={onPosted} />)
  await waitFor(() => {
    expect(screen.getByRole('radio', { name: 'Expense' })).toBeTruthy()
  })
  await userEvent.click(screen.getByRole('radio', { name: 'Expense' }))
  await waitFor(() => {
    expect(screen.getByLabelText('Amount (EUR)')).toBeTruthy()
  })
  await afterRoll()
  await userEvent.type(screen.getByLabelText('Amount (EUR)'), '12.50')
  await userEvent.click(screen.getByRole('button', { name: 'Next' }))
  await waitFor(() => {
    expect(screen.getByLabelText('Category')).toBeTruthy()
  })
  await afterRoll()
  await userEvent.click(screen.getByRole('button', { name: 'Next' }))
  await waitFor(() => {
    expect(screen.getByRole('button', { name: 'Save' })).toBeTruthy()
  })
}

describe('QuickAddPage hidden paint', () => {
  test('Save with Hide set posts then calls entry_set_hidden', async () => {
    await reachSaveStep()
    const hide = screen.getByRole('checkbox', { name: /hide/i })
    expect(hide).not.toBeChecked()
    expect(screen.getByText('Hidden from export')).toBeTruthy()
    expect(screen.queryByRole('radio', { name: /hide/i })).toBeNull()
    expect(screen.queryByRole('checkbox', { name: /include hidden/i })).toBeNull()
    await userEvent.click(hide)
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => {
      expect(api.entryPostSimple).toHaveBeenCalledTimes(1)
    })
    expect(api.entrySetHidden).toHaveBeenCalledWith('j1', true)
  })

  test('Save without Hide does not call entry_set_hidden', async () => {
    await reachSaveStep()
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => {
      expect(api.entryPostSimple).toHaveBeenCalledTimes(1)
    })
    expect(api.entrySetHidden).not.toHaveBeenCalled()
  })

  test('Hide after post fails: retries hide, shows error, does not void', async () => {
    const onPosted = vi.fn()
    vi.mocked(api.entrySetHidden).mockRejectedValue({
      code: 'task_failed',
      message: 'hide failed',
    })
    render(<QuickAddPage onPosted={onPosted} />)
    await waitFor(() => {
      expect(screen.getByRole('radio', { name: 'Expense' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('radio', { name: 'Expense' }))
    await waitFor(() => {
      expect(screen.getByLabelText('Amount (EUR)')).toBeTruthy()
    })
    await afterRoll()
    await userEvent.type(screen.getByLabelText('Amount (EUR)'), '12.50')
    await userEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() => {
      expect(screen.getByLabelText('Category')).toBeTruthy()
    })
    await afterRoll()
    await userEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Save' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('checkbox', { name: /hide/i }))
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => {
      expect(api.entrySetHidden).toHaveBeenCalledTimes(2)
    })
    expect(api.entryPostSimple).toHaveBeenCalledTimes(1)
    expect(api.entryVoid).not.toHaveBeenCalled()
    expect(onPosted).not.toHaveBeenCalled()
    expect(screen.getByRole('alert')).toHaveTextContent('Something went wrong in the background.')
    expect(screen.queryByText('hide failed')).toBeNull()
  })
})

describe('QuickAddPage default accounts', () => {
  // The first expense account is not the default: only Rust's answer says
  // which one is, and neither name contains an English word it could match.
  const second: Account = {
    ...accounts[1],
    id: 'exp2',
    code: '5100',
    name: 'Λογαριασμός 5100',
    sort_order: 2,
  }

  async function categoryAfterChoosingExpense(): Promise<HTMLElement> {
    render(<QuickAddPage onPosted={() => {}} />)
    await waitFor(() => {
      expect(screen.getByRole('radio', { name: 'Expense' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('radio', { name: 'Expense' }))
    await afterRoll()
    await userEvent.type(screen.getByLabelText('Amount (EUR)'), '1')
    await userEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() => {
      expect(screen.getByLabelText('Category')).toBeTruthy()
    })
    return screen.getByLabelText('Category')
  }

  test('the category follows the default Rust returned, not the list order', async () => {
    vi.mocked(api.accountList).mockResolvedValue([...accounts, second])
    vi.mocked(api.accountDefaults).mockResolvedValue({ ...DEFAULTS, category: 'exp2' })

    expect(await categoryAfterChoosingExpense()).toHaveValue('exp2')
    expect(api.accountDefaults).toHaveBeenCalledWith('e1')
  })

  test('remembered last-used accounts still win over the default', async () => {
    vi.mocked(api.accountList).mockResolvedValue([...accounts, second])
    vi.mocked(api.accountDefaults).mockResolvedValue({ ...DEFAULTS, category: 'exp2' })
    vi.mocked(api.getUiPrefs).mockResolvedValue({
      ...prefs,
      last_accounts_by_entity_kind: {
        'e1:expense': {
          category_account_id: 'exp1',
          wallet_account_id: 'w1',
          payable_account_id: null,
          from_account_id: null,
          to_account_id: null,
        },
      },
    })

    expect(await categoryAfterChoosingExpense()).toHaveValue('exp1')
  })
})

describe('QuickAddPage remembering the accounts', () => {
  // Rust refuses to save over a preferences file it could not read. The entry
  // is already posted by then, so the refusal must not look like a failed post.
  test('a refused preferences save does not block the posted entry', async () => {
    const onPosted = vi.fn()
    vi.mocked(api.rememberQuickAdd).mockRejectedValue({
      code: 'serialization',
      message: 'replace a preferences file that does not decode: EOF',
      params: { operation: 'replace a preferences file that does not decode' },
    })
    await reachSaveStep(onPosted)

    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(onPosted).toHaveBeenCalledTimes(1)
    })
    expect(api.entryPostSimple).toHaveBeenCalledTimes(1)
    expect(api.rememberQuickAdd).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('alert')).toBeNull()
  })
})

describe('QuickAddPage post failures', () => {
  test('a failed post with a known code shows the localized copy for it', async () => {
    vi.mocked(api.entryPostSimple).mockReset().mockRejectedValue({
      code: 'account_inactive',
      message: 'account 5100 is inactive',
      params: { code: '5100' },
    })
    await reachSaveStep()

    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Account 5100 is inactive. Choose an active account.',
    )
    expect(screen.queryByText('account 5100 is inactive')).toBeNull()
  })

  test('a failed post with an unknown code never shows the raw message', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.entryPostSimple).mockReset().mockRejectedValue({
      code: 'brand_new',
      message: 'sqlcipher: disk image is malformed',
    })
    await reachSaveStep()

    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('Something went wrong.')
    expect(screen.queryByText(/sqlcipher/)).toBeNull()
  })
})

describe('QuickAddPage unpaid income', () => {
  const receivable: Account = {
    ...accounts[0]!,
    id: 'r1',
    code: '1200',
    name: 'Receivables',
    sort_order: 3,
  }
  const salary: Account = {
    ...accounts[1]!,
    id: 'inc1',
    code: '4000',
    name: 'Salary',
    account_type: 'income',
  }
  const withReceivable: AccountDefaults = { ...DEFAULTS, income: 'inc1', receivable: 'r1' }
  const withoutReceivable: AccountDefaults = { ...withReceivable, receivable: null }

  function bookWith(list: Account[], defaults: AccountDefaults) {
    vi.mocked(api.accountList).mockResolvedValue(list)
    vi.mocked(api.accountDefaults).mockResolvedValue(defaults)
  }

  function postedInput() {
    return vi.mocked(api.entryPostSimple).mock.calls[0]![0]
  }

  async function reachIncomeSaveStep() {
    render(<QuickAddPage onPosted={() => {}} />)
    await userEvent.click(await screen.findByRole('radio', { name: 'Income' }))
    await afterRoll()
    await userEvent.type(await screen.findByLabelText('Amount (EUR)'), '300')
    await userEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() => {
      expect(screen.getByLabelText('Income')).toBeTruthy()
    })
    await afterRoll()
    await userEvent.click(screen.getByRole('button', { name: 'Next' }))
    await screen.findByRole('button', { name: 'Save' })
  }

  /** Drops a file whose reading is `suggestion` and waits for the confirm row. */
  async function dropInvoice(suggestion: Partial<DocumentSuggestion>) {
    vi.mocked(api.documentAnalyze).mockResolvedValue({
      source: 'heuristic',
      model: null,
      kind: 'income',
      amount_minor: 30000,
      entry_date: null,
      description: 'Invoice 7',
      reference: null,
      merchant: null,
      bill_unpaid: true,
      category_account_id: 'inc1',
      wallet_account_id: 'w1',
      payable_account_id: 'r1',
      confidence: 0.9,
      notes: [],
      ...suggestion,
    })
    const { container } = render(<QuickAddPage onPosted={() => {}} />)
    await screen.findByRole('radio', { name: 'Expense' })
    const file = new File(['x'], 'invoice.txt', { type: 'text/plain' })
    fireEvent.drop(container.firstElementChild!, { dataTransfer: { files: [file] } })
    await screen.findByRole('button', { name: 'Save' })
  }

  test('an income is received by default and posts without a status', async () => {
    bookWith([...accounts, salary, receivable], withReceivable)
    await reachIncomeSaveStep()

    expect(screen.getByRole('radio', { name: 'Received' })).toBeChecked()
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.entryPostSimple).toHaveBeenCalledTimes(1))
    expect(postedInput()).toMatchObject({
      kind: 'income',
      bill_status: null,
      wallet_account_id: 'w1',
    })
  })

  test('choosing Unpaid books the income on the default receivable account', async () => {
    bookWith([...accounts, salary, receivable], withReceivable)
    await reachIncomeSaveStep()

    await userEvent.click(screen.getByRole('radio', { name: 'Unpaid' }))
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.entryPostSimple).toHaveBeenCalledTimes(1))
    expect(postedInput()).toMatchObject({
      kind: 'income',
      bill_status: 'unpaid',
      payable_account_id: 'r1',
      category_account_id: 'inc1',
    })
  })

  test('with no receivable account Unpaid is not offered and the income stays received', async () => {
    bookWith([...accounts, salary], withoutReceivable)
    await reachIncomeSaveStep()

    expect(screen.queryByRole('radio', { name: 'Unpaid' })).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.entryPostSimple).toHaveBeenCalledTimes(1))
    expect(postedInput()).toMatchObject({ kind: 'income', bill_status: null })
  })

  test('a dropped invoice with credit terms preselects Unpaid', async () => {
    bookWith([...accounts, salary, receivable], withReceivable)
    await dropInvoice({})

    expect(screen.getByRole('radio', { name: 'Unpaid' })).toBeChecked()
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.entryPostSimpleWithDocument).toHaveBeenCalledTimes(1))
    expect(vi.mocked(api.entryPostSimpleWithDocument).mock.calls[0]![0]).toMatchObject({
      bill_status: 'unpaid',
      payable_account_id: 'r1',
    })
  })

  test('a dropped invoice that is already paid preselects Received', async () => {
    bookWith([...accounts, salary, receivable], withReceivable)
    await dropInvoice({ bill_unpaid: false })

    expect(screen.getByRole('radio', { name: 'Received' })).toBeChecked()
  })

  test('a dropped invoice in a book without a receivable is booked as received', async () => {
    bookWith([...accounts, salary], withoutReceivable)
    await dropInvoice({ payable_account_id: null })

    expect(screen.queryByRole('radio', { name: 'Unpaid' })).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.entryPostSimpleWithDocument).toHaveBeenCalledTimes(1))
    expect(vi.mocked(api.entryPostSimpleWithDocument).mock.calls[0]![0]).toMatchObject({
      kind: 'income',
      bill_status: null,
    })
  })
})
