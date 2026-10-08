import { kindDefaultAccounts, type AccountLike } from './simpleEntry'
import type {
  AccountDefaults,
  CsvColumnMapping,
  CsvImportPreviewRow,
  CsvRequiredColumn,
  LastRoleAccounts,
  SimpleEntryInput,
} from './api'
import { t, type MessageKey } from './i18n'
import type { UiText } from './uiText'

/** True when preview includes a header row to drive mapping selects. */
export function previewHasColumnMap(preview: { headers?: string[] | null }): boolean {
  return (preview.headers?.length ?? 0) > 0
}

/**
 * The sentence that says why the Map columns step has to be filled in: Rust
 * could not detect these required columns, so the preview has no rows yet.
 * `null` when nothing is missing.
 */
export function mapNeededKey(
  missing: readonly CsvRequiredColumn[] | null | undefined,
): MessageKey | null {
  const date = missing?.includes('date') ?? false
  const amount = missing?.includes('amount') ?? false
  if (date && amount) return 'tx.csv.mapNeeded.dateAndAmount'
  if (date) return 'tx.csv.mapNeeded.date'
  if (amount) return 'tx.csv.mapNeeded.amount'
  return null
}

export type CsvMapDraft = {
  date: string
  description: string
  amount: string
  debit: string
  credit: string
  reference: string
  /** Optional, and read only beside a single amount column. */
  direction: string
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
    direction: matchHeader(headers, detected.direction),
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
      // Debit and credit columns already say which way the money moved.
      direction: null,
    }
  }
  return {
    date: orNull(draft.date),
    description: orNull(draft.description),
    amount: orNull(draft.amount),
    debit: null,
    credit: null,
    reference: orNull(draft.reference),
    direction: orNull(draft.direction),
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
    normHeader(a.reference) === normHeader(b.reference) &&
    normHeader(a.direction) === normHeader(b.direction)
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
    a.reference === b.reference &&
    a.direction === b.direction
  )
}

/**
 * Whether core accepts the mapping: date required; amount XOR (debit OR
 * credit). `csvMappingVerdicts.json` pins this to core's rule.
 */
export function mappingReady(mapping: CsvColumnMapping): boolean {
  if (!mapping.date?.trim()) return false
  const amount = Boolean(mapping.amount?.trim())
  const debit = Boolean(mapping.debit?.trim())
  const credit = Boolean(mapping.credit?.trim())
  if (amount) return !debit && !credit
  return debit || credit
}

/** Rows that can be posted: no parse error and a suggested simple entry. */
export function rowSelectable(row: {
  error: UiText | null
  suggested: SimpleEntryInput | null
}): boolean {
  return !row.error && row.suggested != null
}

/** Default checkbox: skip duplicates and junk. */
export function defaultChecked(row: CsvImportPreviewRow): boolean {
  return !row.duplicate && rowSelectable(row)
}

export function postLabel(n: number): string {
  return t('tx.csv.postSelected', { n })
}

export function previewSubtitle(rowCount: number, duplicateCount: number): string {
  const rows = t(rowCount === 1 ? 'tx.csv.rowOne' : 'tx.csv.rowMany', { n: rowCount })
  const dupes = t(duplicateCount === 1 ? 'tx.csv.dupeOne' : 'tx.csv.dupeMany', {
    n: duplicateCount,
  })
  return t('tx.csv.previewSubtitle', { rows, dupes })
}

function orNull(value: string | null | undefined): string | null {
  return value ? value : null
}

/** Wallet + expense/income categories from last-used picks, then form state, then Rust's defaults. */
export function csvImportAccountDefaults(opts: {
  defaults: AccountDefaults | null
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
  const expense = kindDefaultAccounts('expense', opts.defaults)
  const income = kindDefaultAccounts('income', opts.defaults)
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

/**
 * The wallet and category the bulk selectors open on: the accounts the
 * preview rows will post to, so a selector does not show one account while
 * the rows go to another. Falls back to the first listed account when no row
 * carries a listed one.
 */
export function initialBulkAccounts(
  rows: readonly { suggested: SimpleEntryInput | null }[],
  lists: { wallets: readonly AccountLike[]; categories: readonly AccountLike[] },
): { walletId: string; categoryId: string } {
  const pick = (
    field: 'wallet_account_id' | 'category_account_id',
    listed: readonly AccountLike[],
  ): string => {
    const ids = new Set(listed.map((account) => account.id))
    const used = rows.map((row) => row.suggested?.[field]).find((id) => id != null && ids.has(id))
    return used ?? listed[0]?.id ?? ''
  }
  return {
    walletId: pick('wallet_account_id', lists.wallets),
    categoryId: pick('category_account_id', lists.categories),
  }
}
