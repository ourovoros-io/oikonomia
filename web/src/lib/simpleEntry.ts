import type { AccountDefaults, SimpleEntryInput } from './api'
import { t } from './i18n'

export type EntryKind = 'expense' | 'income' | 'bill' | 'transfer'
export type BillStatusTray = 'paid' | 'unpaid'

export type AccountLike = {
  id: string
  name: string
  account_type: 'asset' | 'liability' | 'equity' | 'income' | 'expense'
  is_active: boolean
}

export function lastAccountsMapKey(entityId: string, kind: EntryKind | string): string {
  return `${entityId}:${kind}`
}

export function accountsOf(
  accounts: AccountLike[],
  types: AccountLike['account_type'][],
): AccountLike[] {
  return accounts.filter((a) => a.is_active && types.includes(a.account_type))
}

/** The form fields a kind's default accounts fill, as ids ('' when unset). */
export type KindDefaultAccounts = {
  categoryId: string
  walletId: string
  payableId: string
  fromId: string
  toId: string
}

/**
 * Lays the defaults Rust chose for each role out as the fields of one entry
 * kind. This only routes a role to a form field; which account plays a role is
 * decided in Rust (`account_defaults`), never here.
 */
export function kindDefaultAccounts(
  kind: EntryKind,
  defaults: AccountDefaults | null,
): KindDefaultAccounts {
  const none: KindDefaultAccounts = {
    categoryId: '',
    walletId: '',
    payableId: '',
    fromId: '',
    toId: '',
  }
  if (!defaults) return none

  if (kind === 'expense') {
    return { ...none, categoryId: defaults.category ?? '', walletId: defaults.payment ?? '' }
  }
  if (kind === 'income') {
    // The payable field of an income holds the receivable account, the one
    // an invoice not yet paid is owed on.
    return {
      ...none,
      categoryId: defaults.income ?? '',
      walletId: defaults.deposit ?? '',
      payableId: defaults.receivable ?? '',
    }
  }
  if (kind === 'bill') {
    return {
      ...none,
      categoryId: defaults.bill_category ?? '',
      walletId: defaults.payment ?? '',
      payableId: defaults.bills_payable ?? '',
    }
  }
  return {
    ...none,
    fromId: defaults.transfer_source ?? '',
    toId: defaults.transfer_destination ?? '',
  }
}

/**
 * The status Rust reads: a bill always has one, an income only when it is
 * unpaid (owed on the receivable account), and nothing else has any.
 */
function postedBillStatus(kind: EntryKind, status: BillStatusTray): BillStatusTray | null {
  if (kind === 'bill') return status
  return kind === 'income' && status === 'unpaid' ? 'unpaid' : null
}

export function buildSimpleEntryInput(args: {
  entityId: string
  kind: EntryKind
  billStatus: BillStatusTray
  entryDate: string
  description: string
  amountMinor: number
  categoryId: string
  walletId: string
  payableId: string
  fromId: string
  toId: string
}): SimpleEntryInput {
  return {
    entity_id: args.entityId,
    kind: args.kind,
    bill_status: postedBillStatus(args.kind, args.billStatus),
    entry_date: args.entryDate,
    description: args.description.trim(),
    reference: null,
    amount_minor: args.amountMinor,
    category_account_id: args.categoryId || null,
    wallet_account_id: args.walletId || null,
    payable_account_id: args.payableId || null,
    from_account_id: args.fromId || null,
    to_account_id: args.toId || null,
  }
}

/** Validate role-account selections for the tray quick-add form. */
export function validateTrayAccounts(args: {
  kind: EntryKind
  billStatus: BillStatusTray
  categoryId: string
  walletId: string
  payableId: string
  fromId: string
  toId: string
}): string | null {
  const { kind, billStatus, categoryId, walletId, payableId, fromId, toId } = args
  if (kind === 'expense' && (!categoryId || !walletId)) {
    return t('quickAdd.pickAccounts')
  }
  if (kind === 'income') {
    // An unpaid income is debited to the receivable account, not a wallet.
    const holder = billStatus === 'unpaid' ? payableId : walletId
    if (!categoryId || !holder) return t('quickAdd.pickAccounts')
  }
  if (kind === 'bill') {
    if (!categoryId) return t('quickAdd.pickCategory')
    if (billStatus === 'paid' && !walletId) return t('quickAdd.pickWallet')
    if (billStatus === 'unpaid' && !payableId) return t('quickAdd.pickPayable')
  }
  if (kind === 'transfer') {
    if (!fromId || !toId || fromId === toId) return t('quickAdd.pickDifferentAccounts')
  }
  return null
}
