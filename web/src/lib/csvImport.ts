import { kindDefaultAccounts, type AccountLike } from './simpleEntry'
import type {
  CsvColumnMapping,
  CsvImportPreviewRow,
  LastRoleAccounts,
  SimpleEntryInput,
} from './api'

export const CSV_MAP_FOOTER_NOTE =
  'Unmapped columns are ignored. Debit/credit columns can replace Amount.'

export type CsvMapDraft = {
  date: string
  description: string
  amount: string
  debit: string
  credit: string
  reference: string
  amountMode: 'amount' | 'debit_credit'
}

function eqIgnoreAsciiCase(a: string, b: string): boolean {
  if (a.length !== b.length) return false
  return a.localeCompare(b, 'en', { sensitivity: 'accent' }) === 0
}

/** Pick the file header that matches a detected name (ASCII case-insensitive). */
export function matchHeader(headers: string[], name: string | null | undefined): string {
  const needle = name?.trim()
  if (!needle) return ''
  return headers.find((h) => eqIgnoreAsciiCase(h.trim(), needle)) ?? ''
}

export function amountModeFromMapping(mapping: CsvColumnMapping): CsvMapDraft['amountMode'] {
  const debit = Boolean(mapping.debit?.trim())
  const credit = Boolean(mapping.credit?.trim())
  const amount = Boolean(mapping.amount?.trim())
  if ((debit || credit) && !amount) return 'debit_credit'
  return 'amount'
}

export function draftFromDetected(headers: string[], detected: CsvColumnMapping): CsvMapDraft {
  const amountMode = amountModeFromMapping(detected)
  return {
    date: matchHeader(headers, detected.date),
    description: matchHeader(headers, detected.description),
    amount: amountMode === 'amount' ? matchHeader(headers, detected.amount) : '',
    debit: amountMode === 'debit_credit' ? matchHeader(headers, detected.debit) : '',
    credit: amountMode === 'debit_credit' ? matchHeader(headers, detected.credit) : '',
    reference: matchHeader(headers, detected.reference),
    amountMode,
  }
}

export function draftToMapping(draft: CsvMapDraft): CsvColumnMapping {
  const orNull = (value: string) => (value.trim() ? value : null)
  if (draft.amountMode === 'debit_credit') {
    return {
      date: orNull(draft.date),
      description: orNull(draft.description),
      amount: null,
      debit: orNull(draft.debit),
      credit: orNull(draft.credit),
      reference: orNull(draft.reference),
    }
  }
  return {
    date: orNull(draft.date),
    description: orNull(draft.description),
    amount: orNull(draft.amount),
    debit: null,
    credit: null,
    reference: orNull(draft.reference),
  }
}

function normHeader(value: string | null | undefined): string {
  return (value?.trim() ?? '').toLowerCase()
}

export function mappingsEqual(a: CsvColumnMapping, b: CsvColumnMapping): boolean {
  return (
    normHeader(a.date) === normHeader(b.date) &&
    normHeader(a.description) === normHeader(b.description) &&
    normHeader(a.amount) === normHeader(b.amount) &&
    normHeader(a.debit) === normHeader(b.debit) &&
    normHeader(a.credit) === normHeader(b.credit) &&
    normHeader(a.reference) === normHeader(b.reference)
  )
}

export function draftsEqual(a: CsvMapDraft, b: CsvMapDraft): boolean {
  return (
    a.amountMode === b.amountMode &&
    a.date === b.date &&
    a.description === b.description &&
    a.amount === b.amount &&
    a.debit === b.debit &&
    a.credit === b.credit &&
    a.reference === b.reference
  )
}

/** date + description required; amount XOR (debit AND credit). */
export function mappingReady(mapping: CsvColumnMapping): boolean {
  if (!mapping.date?.trim() || !mapping.description?.trim()) return false
  const amount = Boolean(mapping.amount?.trim())
  const debit = Boolean(mapping.debit?.trim())
  const credit = Boolean(mapping.credit?.trim())
  if (amount) return !debit && !credit
  return debit && credit
}

/** Rows that can be posted: no parse error and a suggested simple entry. */
export function rowSelectable(row: {
  error: string | null
  suggested: SimpleEntryInput | null
}): boolean {
  return !row.error && row.suggested != null
}

/** Default checkbox: skip duplicates and junk. */
export function defaultChecked(row: CsvImportPreviewRow): boolean {
  return !row.duplicate && rowSelectable(row)
}

export function postLabel(n: number): string {
  return `Post ${n} selected`
}

export function previewSubtitle(rowCount: number, duplicateCount: number): string {
  const rows = `${rowCount} ${rowCount === 1 ? 'row' : 'rows'}`
  const dupes = `${duplicateCount} likely duplicate${duplicateCount === 1 ? '' : 's'} flagged`
  return `${rows} · ${dupes} · nothing posts until you confirm`
}

function orNull(value: string | null | undefined): string | null {
  return value ? value : null
}

/** Wallet + expense/income categories from last-used picks, then form state, then name hints. */
export function csvImportAccountDefaults(opts: {
  accounts: AccountLike[]
  walletId: string
  categoryId: string
  kind: 'expense' | 'income' | 'bill' | 'transfer'
  expenseLast?: LastRoleAccounts | null
  incomeLast?: LastRoleAccounts | null
}): {
  wallet_account_id: string | null
  expense_account_id: string | null
  income_account_id: string | null
} {
  const expense = kindDefaultAccounts('expense', opts.accounts)
  const income = kindDefaultAccounts('income', opts.accounts)
  return {
    wallet_account_id: orNull(
      opts.expenseLast?.wallet_account_id || opts.walletId || expense.walletId,
    ),
    expense_account_id: orNull(
      opts.expenseLast?.category_account_id ||
        (opts.kind === 'expense' ? opts.categoryId : '') ||
        expense.categoryId,
    ),
    income_account_id: orNull(
      opts.incomeLast?.category_account_id ||
        (opts.kind === 'income' ? opts.categoryId : '') ||
        income.categoryId,
    ),
  }
}

/** Apply bulk wallet/category to a suggested row. Category follows expense vs income kind. */
export function applyBulkAccounts(
  suggested: SimpleEntryInput,
  bulk: { walletId: string; categoryId: string; categoryType: AccountLike['account_type'] | null },
): SimpleEntryInput {
  const next = { ...suggested }
  if (bulk.walletId) next.wallet_account_id = bulk.walletId
  if (
    bulk.categoryId &&
    bulk.categoryType &&
    (suggested.kind === 'expense' || suggested.kind === 'income') &&
    suggested.kind === bulk.categoryType
  ) {
    next.category_account_id = bulk.categoryId
  }
  return next
}
