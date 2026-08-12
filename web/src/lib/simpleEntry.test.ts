import { describe, expect, test } from 'vitest'
import {
  buildSimpleEntryInput,
  kindDefaultAccounts,
  lastAccountsMapKey,
  validateTrayAccounts,
  type AccountLike,
} from './simpleEntry'

const accounts: AccountLike[] = [
  { id: 'e1', name: 'Food', account_type: 'expense', is_active: true },
  { id: 'i1', name: 'Salary', account_type: 'income', is_active: true },
  { id: 'a1', name: 'Checking', account_type: 'asset', is_active: true },
  { id: 'a2', name: 'Savings', account_type: 'asset', is_active: true },
  { id: 'l1', name: 'Bills Payable', account_type: 'liability', is_active: true },
]

describe('lastAccountsMapKey', () => {
  test('joins entity and kind', () => {
    expect(lastAccountsMapKey('ent', 'expense')).toBe('ent:expense')
  })
})

describe('kindDefaultAccounts', () => {
  test('expense picks expense + checking', () => {
    const d = kindDefaultAccounts('expense', accounts)
    expect(d.categoryId).toBe('e1')
    expect(d.walletId).toBe('a1')
  })

  test('transfer picks two assets', () => {
    const d = kindDefaultAccounts('transfer', accounts)
    expect(d.fromId).toBe('a1')
    expect(d.toId).toBe('a2')
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

describe('validateTrayAccounts', () => {
  const base = {
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
      'Pick accounts',
    )
    expect(validateTrayAccounts({ ...base, kind: 'expense', walletId: '' })).toBe(
      'Pick accounts',
    )
  })

  test('income requires both accounts', () => {
    expect(validateTrayAccounts({ ...base, kind: 'income', walletId: '' })).toBe(
      'Pick accounts',
    )
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
    ).toBe('Pick category')
    expect(
      validateTrayAccounts({
        ...base,
        kind: 'bill',
        billStatus: 'unpaid',
        payableId: '',
      }),
    ).toBe('Pick payable')
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
    ).toBe('Pick wallet')
  })

  test('transfer needs distinct from/to', () => {
    expect(validateTrayAccounts({ ...base, kind: 'transfer' })).toBeNull()
    expect(
      validateTrayAccounts({ ...base, kind: 'transfer', fromId: '' }),
    ).toBe('Pick different accounts')
    expect(
      validateTrayAccounts({
        ...base,
        kind: 'transfer',
        fromId: 'a1',
        toId: 'a1',
      }),
    ).toBe('Pick different accounts')
  })
})
