import { describe, expect, test } from 'vitest'
import type { AccountDefaults, CsvColumnMapping, CsvImportPreviewRow, SimpleEntryInput } from './api'
import {
  applyBulkAccounts,
  csvImportAccountDefaults,
  defaultChecked,
  draftFromDetected,
  draftToMapping,
  mapNeededKey,
  mappingsEqual,
  mappingReady,
  matchHeader,
  postLabel,
  previewHasColumnMap,
  previewSubtitle,
  rowSelectable,
} from './csvImport'

/** What Rust's `account_defaults` returns for a book; the UI never picks these. */
const defaults: AccountDefaults = {
  category: 'exp1',
  payment: 'w1',
  deposit: 'w1',
  income: 'inc1',
  bill_category: 'exp1',
  bills_payable: null,
  transfer_source: 'w1',
  transfer_destination: 'w1',
}

function suggested(kind: 'expense' | 'income'): SimpleEntryInput {
  return {
    entity_id: 'e1',
    kind,
    bill_status: null,
    entry_date: '2026-08-12',
    description: 'Alpha',
    reference: null,
    amount_minor: 4250,
    category_account_id: kind === 'expense' ? 'exp1' : 'inc1',
    wallet_account_id: 'w1',
    payable_account_id: null,
    from_account_id: null,
    to_account_id: null,
  }
}

const INVALID_AMOUNT = { code: 'csv_invalid_amount', params: { value: 'abc' } }

function row(over: Partial<CsvImportPreviewRow> & { suggested?: SimpleEntryInput | null }): CsvImportPreviewRow {
  return {
    source_row: 2,
    duplicate: false,
    error: null,
    suggested: suggested('expense'),
    signed_amount_minor: -4250,
    ...over,
  }
}

describe('defaultChecked', () => {
  test('checks a clean row', () => {
    expect(defaultChecked(row({}))).toBe(true)
  })

  test('unchecks duplicates', () => {
    expect(defaultChecked(row({ duplicate: true }))).toBe(false)
  })

  test('unchecks error rows', () => {
    expect(defaultChecked(row({ error: INVALID_AMOUNT, suggested: null }))).toBe(false)
  })
})

describe('rowSelectable', () => {
  test('junk rows cannot be selected', () => {
    expect(rowSelectable(row({ error: INVALID_AMOUNT, suggested: null }))).toBe(false)
    expect(rowSelectable(row({ error: null, suggested: null }))).toBe(false)
    expect(rowSelectable(row({}))).toBe(true)
  })
})

describe('postLabel', () => {
  test('matches checked count', () => {
    expect(postLabel(0)).toBe('Post 0 selected')
    expect(postLabel(4)).toBe('Post 4 selected')
  })
})

describe('previewSubtitle', () => {
  test('counts rows and duplicates', () => {
    expect(previewSubtitle(12, 2)).toBe(
      '12 rows · 2 likely duplicates flagged · nothing posts until you confirm',
    )
  })
})

describe('csvImportAccountDefaults', () => {
  test('falls back to the defaults Rust returned when form and last-accounts are empty', () => {
    expect(
      csvImportAccountDefaults({
        defaults,
        walletId: '',
        categoryId: '',
        kind: 'expense',
      }),
    ).toEqual({
      wallet_account_id: 'w1',
      expense_account_id: 'exp1',
      income_account_id: 'inc1',
    })
  })

  test('prefers last-accounts over form state', () => {
    expect(
      csvImportAccountDefaults({
        defaults,
        walletId: 'form-wallet',
        categoryId: 'form-exp',
        kind: 'expense',
        expenseLast: {
          category_account_id: 'last-exp',
          wallet_account_id: 'last-wallet',
          payable_account_id: null,
          from_account_id: null,
          to_account_id: null,
        },
        incomeLast: {
          category_account_id: 'last-inc',
          wallet_account_id: null,
          payable_account_id: null,
          from_account_id: null,
          to_account_id: null,
        },
      }),
    ).toEqual({
      wallet_account_id: 'last-wallet',
      expense_account_id: 'last-exp',
      income_account_id: 'last-inc',
    })
  })
})

describe('applyBulkAccounts', () => {
  test('writes wallet on every row and category only when kind matches', () => {
    const expense = applyBulkAccounts(suggested('expense'), {
      walletId: 'w2',
      categoryId: 'exp2',
      categoryType: 'expense',
    })
    expect(expense.wallet_account_id).toBe('w2')
    expect(expense.category_account_id).toBe('exp2')

    const income = applyBulkAccounts(suggested('income'), {
      walletId: 'w2',
      categoryId: 'exp2',
      categoryType: 'expense',
    })
    expect(income.wallet_account_id).toBe('w2')
    expect(income.category_account_id).toBe('inc1')
  })
})

describe('column mapping helpers', () => {
  const headers = ['Date', 'Payee', 'Notes', 'Amount']
  const detected: CsvColumnMapping = {
    date: 'Date',
    description: 'Payee',
    amount: 'Amount',
    debit: null,
    credit: null,
    reference: null,
  }

  test('matchHeader is ASCII case-insensitive', () => {
    expect(matchHeader(headers, 'payee')).toBe('Payee')
    expect(matchHeader(headers, 'DATE')).toBe('Date')
  })

  test('draftFromDetected pre-fills header names', () => {
    const draft = draftFromDetected(headers, detected)
    expect(draft.date).toBe('Date')
    expect(draft.description).toBe('Payee')
    expect(draft.amount).toBe('Amount')
    expect(draft.amountMode).toBe('amount')
  })

  test('mappingReady requires date, description, and amount XOR debit+credit', () => {
    expect(mappingReady(detected)).toBe(true)
    expect(mappingReady({ ...detected, date: null })).toBe(false)
    expect(mappingReady({ ...detected, amount: null, debit: 'Out', credit: 'In' })).toBe(true)
    expect(mappingReady({ ...detected, amount: 'Amount', debit: 'Out', credit: 'In' })).toBe(false)
    expect(mappingReady({ ...detected, amount: null, debit: 'Out', credit: null })).toBe(false)
  })

  test('draftToMapping keeps chosen headers', () => {
    const mapping = draftToMapping({
      date: 'Date',
      description: 'Notes',
      amount: 'Amount',
      debit: '',
      credit: '',
      reference: '',
      direction: '',
      amountMode: 'amount',
    })
    expect(mapping.description).toBe('Notes')
    expect(mapping.amount).toBe('Amount')
    expect(mapping.debit).toBeNull()
  })

  test('an edited mapping keeps the detected direction column', () => {
    const typed = [...headers, 'Type']
    const draft = draftFromDetected(typed, { ...detected, direction: 'type' })
    expect(draft.direction).toBe('Type')

    const edited = draftToMapping({ ...draft, description: 'Notes' })
    expect(edited.direction).toBe('Type')
    expect(edited.description).toBe('Notes')
  })

  test('debit and credit columns drop the direction column', () => {
    const draft = draftFromDetected([...headers, 'Type'], { ...detected, direction: 'Type' })
    const mapping = draftToMapping({
      ...draft,
      amountMode: 'debit_credit',
      amount: '',
      debit: 'Out',
      credit: 'In',
    })
    expect(mapping.direction).toBeNull()
  })

  test('mappingsEqual compares the direction column', () => {
    expect(mappingsEqual(detected, { ...detected, direction: null })).toBe(true)
    expect(mappingsEqual(detected, { ...detected, direction: 'Type' })).toBe(false)
  })

  test('mappingsEqual ignores ASCII case', () => {
    expect(mappingsEqual(detected, { ...detected, date: 'date' })).toBe(true)
    expect(mappingsEqual(detected, { ...detected, description: 'Notes' })).toBe(false)
  })

  test('previewHasColumnMap is false until headers exist', () => {
    expect(previewHasColumnMap({ headers: ['Date'] })).toBe(true)
    expect(previewHasColumnMap({ headers: [] })).toBe(false)
    expect(previewHasColumnMap({})).toBe(false)
  })
})

describe('mapNeededKey', () => {
  test('names the missing column, or both, whatever order they come in', () => {
    expect(mapNeededKey(['date'])).toBe('tx.csv.mapNeeded.date')
    expect(mapNeededKey(['amount'])).toBe('tx.csv.mapNeeded.amount')
    expect(mapNeededKey(['date', 'amount'])).toBe('tx.csv.mapNeeded.dateAndAmount')
    expect(mapNeededKey(['amount', 'date'])).toBe('tx.csv.mapNeeded.dateAndAmount')
  })

  test('is null when nothing is missing or the preview does not say', () => {
    expect(mapNeededKey([])).toBeNull()
    expect(mapNeededKey(undefined)).toBeNull()
    expect(mapNeededKey(null)).toBeNull()
  })
})
