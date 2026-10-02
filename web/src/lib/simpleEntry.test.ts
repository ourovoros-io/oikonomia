import { afterEach, describe, expect, test } from 'vitest'
import type { AccountDefaults } from './api'
import { resetI18nForTests, t } from './i18n'
import {
  buildSimpleEntryInput,
  kindDefaultAccounts,
  lastAccountsMapKey,
  validateTrayAccounts,
} from './simpleEntry'

/** What Rust's `account_defaults` returns; the names here are deliberately not English. */
const defaults: AccountDefaults = {
  category: 'e1',
  payment: 'a1',
  deposit: 'a3',
  income: 'i1',
  bill_category: 'e2',
  bills_payable: 'l1',
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
      t('quickAdd.pickAccounts'),
    )
    expect(validateTrayAccounts({ ...base, kind: 'expense', walletId: '' })).toBe(
      t('quickAdd.pickAccounts'),
    )
  })

  test('income requires both accounts', () => {
    expect(validateTrayAccounts({ ...base, kind: 'income', walletId: '' })).toBe(
      t('quickAdd.pickAccounts'),
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
    ).toBe(t('quickAdd.pickCategory'))
    expect(
      validateTrayAccounts({
        ...base,
        kind: 'bill',
        billStatus: 'unpaid',
        payableId: '',
      }),
    ).toBe(t('quickAdd.pickPayable'))
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
    ).toBe(t('quickAdd.pickWallet'))
  })

  test('transfer needs distinct from/to', () => {
    expect(validateTrayAccounts({ ...base, kind: 'transfer' })).toBeNull()
    expect(
      validateTrayAccounts({ ...base, kind: 'transfer', fromId: '' }),
    ).toBe(t('quickAdd.pickDifferentAccounts'))
    expect(
      validateTrayAccounts({
        ...base,
        kind: 'transfer',
        fromId: 'a1',
        toId: 'a1',
      }),
    ).toBe(t('quickAdd.pickDifferentAccounts'))
  })
})
