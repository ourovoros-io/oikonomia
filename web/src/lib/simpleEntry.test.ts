import { afterEach, describe, expect, test } from 'vitest'
import type { AccountDefaults } from './api'
import { resetI18nForTests } from './i18n'
import {
  accountChoices,
  buildSimpleEntryInput,
  formBillStatus,
  holdsUnusableAccount,
  isBillStatus,
  kindDefaultAccounts,
  lastAccountsMapKey,
  shownAccountFields,
  validateTrayAccounts,
  type AccountLike,
} from './simpleEntry'

/** What Rust's `account_defaults` returns; the names here are deliberately not English. */
const defaults: AccountDefaults = {
  category: 'e1',
  payment: 'a1',
  deposit: 'a3',
  income: 'i1',
  bill_category: 'e2',
  bills_payable: 'l1',
  receivable: 'r1',
  transfer_source: 'a1',
  transfer_destination: 'a2',
}

afterEach(() => {
  resetI18nForTests()
})

describe('lastAccountsMapKey', () => {
  test('joins entity and kind', () => {
    expect(lastAccountsMapKey('ent', 'expense')).toBe('ent:expense')
  })
})

describe('kindDefaultAccounts', () => {
  test('expense follows the category and payment roles Rust returned', () => {
    expect(kindDefaultAccounts('expense', defaults)).toEqual({
      categoryId: 'e1',
      walletId: 'a1',
      payableId: '',
      fromId: '',
      toId: '',
    })
  })

  test('income follows the income and deposit roles', () => {
    const d = kindDefaultAccounts('income', defaults)
    expect(d.categoryId).toBe('i1')
    expect(d.walletId).toBe('a3')
    expect(d.payableId).toBe('r1')
  })

  test('bill follows the bill category, payment and payable roles', () => {
    const d = kindDefaultAccounts('bill', defaults)
    expect(d.categoryId).toBe('e2')
    expect(d.walletId).toBe('a1')
    expect(d.payableId).toBe('l1')
  })

  test('transfer follows the source and destination roles', () => {
    const d = kindDefaultAccounts('transfer', defaults)
    expect(d.fromId).toBe('a1')
    expect(d.toId).toBe('a2')
  })

  test('a role Rust left null stays empty, and no defaults at all leaves every field empty', () => {
    const d = kindDefaultAccounts('bill', { ...defaults, bills_payable: null })
    expect(d.payableId).toBe('')

    expect(kindDefaultAccounts('expense', null)).toEqual({
      categoryId: '',
      walletId: '',
      payableId: '',
      fromId: '',
      toId: '',
    })
  })
})

describe('buildSimpleEntryInput', () => {
  test('maps expense fields and null bill_status', () => {
    const input = buildSimpleEntryInput({
      entityId: 'ent',
      kind: 'expense',
      billStatus: 'unpaid',
      entryDate: '2026-08-11',
      description: ' Coffee ',
      amountMinor: 250,
      categoryId: 'e1',
      walletId: 'a1',
      payableId: '',
      fromId: '',
      toId: '',
    })
    expect(input.kind).toBe('expense')
    expect(input.bill_status).toBeNull()
    expect(input.description).toBe('Coffee')
    expect(input.category_account_id).toBe('e1')
    expect(input.wallet_account_id).toBe('a1')
  })

  test('bill includes unpaid status only', () => {
    const input = buildSimpleEntryInput({
      entityId: 'ent',
      kind: 'bill',
      billStatus: 'unpaid',
      entryDate: '2026-08-11',
      description: 'Gas',
      amountMinor: 1000,
      categoryId: 'e1',
      walletId: 'a1',
      payableId: 'l1',
      fromId: '',
      toId: '',
    })
    expect(input.bill_status).toBe('unpaid')
    expect(input.payable_account_id).toBe('l1')
  })
})

describe('buildSimpleEntryInput income status', () => {
  const income = {
    entityId: 'ent',
    kind: 'income' as const,
    entryDate: '2026-08-11',
    description: 'Invoice 7',
    amountMinor: 30000,
    categoryId: 'i1',
    walletId: 'a3',
    payableId: 'r1',
    fromId: '',
    toId: '',
  }

  test('an unpaid income posts the unpaid status and its receivable', () => {
    const input = buildSimpleEntryInput({ ...income, billStatus: 'unpaid' })
    expect(input.bill_status).toBe('unpaid')
    expect(input.payable_account_id).toBe('r1')
  })

  test('a received income posts no status', () => {
    expect(buildSimpleEntryInput({ ...income, billStatus: 'paid' }).bill_status).toBeNull()
  })
})

describe('validateTrayAccounts', () => {
  const accounts: AccountLike[] = [
    { id: 'e1', name: 'Meals', account_type: 'expense', is_active: true },
    { id: 'e0', name: 'Old meals', account_type: 'expense', is_active: false },
    { id: 'i1', name: 'Salary', account_type: 'income', is_active: true },
    { id: 'a1', name: 'Checking', account_type: 'asset', is_active: true },
    { id: 'a2', name: 'Savings', account_type: 'asset', is_active: true },
    { id: 'r1', name: 'Receivables', account_type: 'asset', is_active: true },
    { id: 'l1', name: 'Payable', account_type: 'liability', is_active: true },
  ]
  const base = {
    accounts,
    billStatus: 'unpaid' as const,
    categoryId: 'e1',
    walletId: 'a1',
    payableId: 'l1',
    fromId: 'a1',
    toId: 'a2',
  }

  test('expense ok when category and wallet set', () => {
    expect(validateTrayAccounts({ ...base, kind: 'expense' })).toBeNull()
  })

  test('expense requires both accounts', () => {
    expect(validateTrayAccounts({ ...base, kind: 'expense', categoryId: '' })).toBe(
      'quickAdd.pickAccounts',
    )
    expect(validateTrayAccounts({ ...base, kind: 'expense', walletId: '' })).toBe(
      'quickAdd.pickAccounts',
    )
  })

  test('a held account its pill does not list asks for accounts', () => {
    const expense = { ...base, kind: 'expense' as const }
    expect(validateTrayAccounts({ ...expense, categoryId: 'e0' })).toBe('quickAdd.pickAccounts')
    expect(validateTrayAccounts({ ...expense, categoryId: 'a1' })).toBe('quickAdd.pickAccounts')
    expect(validateTrayAccounts({ ...expense, walletId: 'gone' })).toBe('quickAdd.pickAccounts')

    const received = { ...base, kind: 'income' as const, billStatus: 'paid' as const }
    expect(validateTrayAccounts({ ...received, categoryId: 'i1', walletId: 'l1' })).toBe(
      'quickAdd.pickAccounts',
    )
    // A field the kind does not show is never checked.
    expect(validateTrayAccounts({ ...expense, payableId: 'gone', toId: 'gone' })).toBeNull()

    const owed = { ...base, kind: 'income' as const, categoryId: 'i1', payableId: 'l1' }
    expect(validateTrayAccounts(owed)).toBe('quickAdd.pickAccounts')
    const bill = { ...base, kind: 'bill' as const, billStatus: 'unpaid' as const }
    expect(validateTrayAccounts({ ...bill, payableId: 'a1' })).toBe('quickAdd.pickAccounts')
    const transfer = { ...base, kind: 'transfer' as const }
    expect(validateTrayAccounts({ ...transfer, toId: 'e1' })).toBe('quickAdd.pickAccounts')
  })

  test('a received income requires a category and a wallet', () => {
    const received = {
      ...base,
      kind: 'income' as const,
      billStatus: 'paid' as const,
      categoryId: 'i1',
    }
    expect(validateTrayAccounts(received)).toBeNull()
    expect(validateTrayAccounts({ ...received, walletId: '' })).toBe('quickAdd.pickAccounts')
  })

  test('an unpaid income requires a category and a receivable, not a wallet', () => {
    const unpaid = {
      ...base,
      kind: 'income' as const,
      billStatus: 'unpaid' as const,
      categoryId: 'i1',
      payableId: 'r1',
    }
    expect(validateTrayAccounts({ ...unpaid, walletId: '' })).toBeNull()
    expect(validateTrayAccounts({ ...unpaid, payableId: '' })).toBe('quickAdd.pickAccounts')
  })

  test('bill unpaid needs category + payable', () => {
    expect(
      validateTrayAccounts({ ...base, kind: 'bill', billStatus: 'unpaid' }),
    ).toBeNull()
    expect(
      validateTrayAccounts({
        ...base,
        kind: 'bill',
        billStatus: 'unpaid',
        categoryId: '',
      }),
    ).toBe('quickAdd.pickCategory')
    expect(
      validateTrayAccounts({
        ...base,
        kind: 'bill',
        billStatus: 'unpaid',
        payableId: '',
      }),
    ).toBe('quickAdd.pickPayable')
  })

  test('bill paid needs category + wallet', () => {
    expect(
      validateTrayAccounts({ ...base, kind: 'bill', billStatus: 'paid' }),
    ).toBeNull()
    expect(
      validateTrayAccounts({
        ...base,
        kind: 'bill',
        billStatus: 'paid',
        walletId: '',
      }),
    ).toBe('quickAdd.pickWallet')
  })

  test('transfer needs distinct from/to', () => {
    expect(validateTrayAccounts({ ...base, kind: 'transfer' })).toBeNull()
    expect(
      validateTrayAccounts({ ...base, kind: 'transfer', fromId: '' }),
    ).toBe('quickAdd.pickDifferentAccounts')
    expect(
      validateTrayAccounts({
        ...base,
        kind: 'transfer',
        fromId: 'a1',
        toId: 'a1',
      }),
    ).toBe('quickAdd.pickDifferentAccounts')
  })
})

describe('formBillStatus', () => {
  test('a bill sends its status as chosen', () => {
    expect(formBillStatus('bill', 'paid', false)).toBe('paid')
    expect(formBillStatus('bill', 'unpaid', true)).toBe('unpaid')
    expect(formBillStatus('bill', 'pay_existing', false)).toBe('pay_existing')
  })

  test('an income sends unpaid only when it is owed, whatever the bill field holds', () => {
    expect(formBillStatus('income', 'pay_existing', true)).toBe('unpaid')
    expect(formBillStatus('income', 'unpaid', false)).toBeNull()
  })

  test('an expense and a transfer send no status', () => {
    expect(formBillStatus('expense', 'unpaid', true)).toBeNull()
    expect(formBillStatus('transfer', 'pay_existing', true)).toBeNull()
  })
})

describe('isBillStatus', () => {
  test('accepts the three statuses Rust names and nothing else', () => {
    expect(['paid', 'unpaid', 'pay_existing'].every(isBillStatus)).toBe(true)
    expect(isBillStatus('Paid')).toBe(false)
    expect(isBillStatus('')).toBe(false)
  })
})

describe('account fields', () => {
  const book: AccountLike[] = [
    { id: 'a1', name: 'Bank', account_type: 'asset', is_active: true },
    { id: 'a0', name: 'Old bank', account_type: 'asset', is_active: false },
    { id: 'e1', name: 'Food', account_type: 'expense', is_active: true },
    { id: 'e0', name: 'Old food', account_type: 'expense', is_active: false },
    { id: 'l1', name: 'Payable', account_type: 'liability', is_active: true },
  ]
  const held = { category: 'e1', wallet: 'a1', payable: 'l1', from: 'a1', to: 'l1' }
  const ids = (list: readonly AccountLike[]) => list.map((a) => a.id)

  test('each kind and status shows the fields core posts to', () => {
    expect(shownAccountFields('expense', 'paid', false)).toEqual(['category', 'wallet'])
    expect(shownAccountFields('income', 'paid', false)).toEqual(['category', 'wallet'])
    expect(shownAccountFields('income', 'paid', true)).toEqual(['category', 'payable'])
    expect(shownAccountFields('bill', 'paid', false)).toEqual(['category', 'wallet'])
    expect(shownAccountFields('bill', 'unpaid', false)).toEqual(['category', 'payable'])
    expect(shownAccountFields('bill', 'pay_existing', false)).toEqual(['wallet', 'payable'])
    expect(shownAccountFields('transfer', 'paid', false)).toEqual(['from', 'to'])
  })

  test('a select lists the active accounts of its types, plus an archived one it holds', () => {
    expect(ids(accountChoices(book, 'expense', 'category', 'e1'))).toEqual(['e1'])
    expect(ids(accountChoices(book, 'expense', 'category', 'e0'))).toEqual(['e1', 'e0'])
    expect(ids(accountChoices(book, 'expense', 'wallet', ''))).toEqual(['a1', 'l1'])
    expect(ids(accountChoices(book, 'income', 'wallet', 'l1'))).toEqual(['a1'])
    expect(ids(accountChoices(book, 'expense', 'from', 'a1'))).toEqual([])
  })

  test('a shown field that is empty, archived or of the wrong type holds the save', () => {
    const form = { kind: 'expense', billStatus: 'paid', incomeUnpaid: false } as const

    expect(holdsUnusableAccount(book, { ...form, held })).toBe(false)
    expect(holdsUnusableAccount(book, { ...form, held: { ...held, category: '' } })).toBe(true)
    expect(holdsUnusableAccount(book, { ...form, held: { ...held, category: 'e0' } })).toBe(true)
    expect(holdsUnusableAccount(book, { ...form, held: { ...held, category: 'a1' } })).toBe(true)
    expect(holdsUnusableAccount(book, { ...form, held: { ...held, category: 'gone' } })).toBe(true)
  })

  test('a hidden field never holds the save', () => {
    const unpaidBill = { kind: 'bill', billStatus: 'unpaid', incomeUnpaid: false } as const

    expect(holdsUnusableAccount(book, { ...unpaidBill, held: { ...held, wallet: 'a0' } })).toBe(
      false,
    )
    expect(holdsUnusableAccount(book, { ...unpaidBill, held: { ...held, payable: '' } })).toBe(true)
  })
})
