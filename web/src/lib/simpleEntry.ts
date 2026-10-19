import type { AccountDefaults, SimpleEntryInput } from './api'
import type { MessageKey } from './i18n'

export type EntryKind = 'expense' | 'income' | 'bill' | 'transfer'
export type BillStatusTray = 'paid' | 'unpaid'
/** The payment state of a bill as Rust names it; an unpaid income uses `unpaid` too. */
export type BillStatus = NonNullable<SimpleEntryInput['bill_status']>

const BILL_STATUSES: readonly string[] = ['paid', 'unpaid', 'pay_existing'] satisfies BillStatus[]

/** Narrows a select's value to a bill status. */
export function isBillStatus(value: string): value is BillStatus {
  return BILL_STATUSES.includes(value)
}

export type AccountLike = {
  id: string
  name: string
  account_type: 'asset' | 'liability' | 'equity' | 'income' | 'expense'
  is_active: boolean
}

export function lastAccountsMapKey(entityId: string, kind: EntryKind | string): string {
  return `${entityId}:${kind}`
}

/** An account field of the entry forms; `payable` holds the receivable of an income. */
export type AccountFieldName = 'category' | 'wallet' | 'payable' | 'from' | 'to'

type AccountType = AccountLike['account_type']

const WALLET_TYPES: readonly AccountType[] = ['asset', 'liability']

/** The account types each field of a kind lists. Core checks the type again on save. */
const FIELD_TYPES: Record<EntryKind, Partial<Record<AccountFieldName, readonly AccountType[]>>> = {
  expense: { category: ['expense'], wallet: WALLET_TYPES },
  income: { category: ['income'], wallet: ['asset'], payable: ['asset'] },
  bill: { category: ['expense'], wallet: WALLET_TYPES, payable: ['liability'] },
  transfer: { from: WALLET_TYPES, to: WALLET_TYPES },
}

function fieldTypes(kind: EntryKind, field: AccountFieldName): readonly AccountType[] {
  return FIELD_TYPES[kind][field] ?? []
}

/**
 * The account fields a form shows for this kind and status, the ones core
 * posts to. Every other field is hidden and its value ignored.
 */
export function shownAccountFields(
  kind: EntryKind,
  billStatus: BillStatus,
  incomeUnpaid: boolean,
): readonly AccountFieldName[] {
  if (kind === 'expense') return ['category', 'wallet']
  if (kind === 'income') return ['category', incomeUnpaid ? 'payable' : 'wallet']
  if (kind === 'transfer') return ['from', 'to']
  if (billStatus === 'paid') return ['category', 'wallet']
  if (billStatus === 'unpaid') return ['category', 'payable']
  return ['wallet', 'payable']
}

/**
 * What a field's select lists: the active accounts of its types, and the
 * account it holds when that one is archived, so the select can show it as
 * held rather than fall back to its first option.
 */
export function accountChoices<A extends AccountLike>(
  accounts: readonly A[],
  kind: EntryKind,
  field: AccountFieldName,
  heldId: string,
): A[] {
  const types = fieldTypes(kind, field)

  return accounts.filter((a) => types.includes(a.account_type) && (a.is_active || a.id === heldId))
}

/**
 * True when a shown field holds no account it can post to: none picked, an
 * archived one, or one of the wrong type. Core would refuse the save, so the
 * form holds it back until the field is picked again.
 */
export function holdsUnusableAccount<A extends AccountLike>(
  accounts: readonly A[],
  form: {
    kind: EntryKind
    billStatus: BillStatus
    incomeUnpaid: boolean
    held: Record<AccountFieldName, string>
  },
): boolean {
  return shownAccountFields(form.kind, form.billStatus, form.incomeUnpaid).some((field) => {
    const id = form.held[field]
    const types = fieldTypes(form.kind, field)
    return !accounts.some((a) => a.id === id && a.is_active && types.includes(a.account_type))
  })
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
 * What a form with a bill status field and an income "received or owed"
 * field tells Rust: the bill's status, `unpaid` for an income owed to the
 * book, and nothing for any other kind. Rust decides what each pair posts.
 */
export function formBillStatus(
  kind: EntryKind,
  billStatus: BillStatus,
  incomeUnpaid: boolean,
): BillStatus | null {
  if (kind === 'bill') return billStatus
  return kind === 'income' && incomeUnpaid ? 'unpaid' : null
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
    bill_status: formBillStatus(args.kind, args.billStatus, args.billStatus === 'unpaid'),
    entry_date: args.entryDate,
    description: args.description.trim(),
    reference: null,
    amount_minor: args.amountMinor,
    category_account_id: args.categoryId || null,
    wallet_account_id: args.walletId || null,
    payable_account_id: args.payableId || null,
    from_account_id: args.fromId || null,
    to_account_id: args.toId || null,
    settles_entry_id: null,
  }
}

/**
 * Validate role-account selections for the tray quick-add form: the catalog key
 * of what is missing, or null. A key, not a sentence, so the banner words it.
 */
export function validateTrayAccounts(args: {
  kind: EntryKind
  billStatus: BillStatusTray
  categoryId: string
  walletId: string
  payableId: string
  fromId: string
  toId: string
}): MessageKey | null {
  const { kind, billStatus, categoryId, walletId, payableId, fromId, toId } = args
  if (kind === 'expense' && (!categoryId || !walletId)) {
    return 'quickAdd.pickAccounts'
  }
  if (kind === 'income') {
    // An unpaid income is debited to the receivable account, not a wallet.
    const holder = billStatus === 'unpaid' ? payableId : walletId
    if (!categoryId || !holder) return 'quickAdd.pickAccounts'
  }
  if (kind === 'bill') {
    if (!categoryId) return 'quickAdd.pickCategory'
    if (billStatus === 'paid' && !walletId) return 'quickAdd.pickWallet'
    if (billStatus === 'unpaid' && !payableId) return 'quickAdd.pickPayable'
  }
  if (kind === 'transfer') {
    if (!fromId || !toId || fromId === toId) return 'quickAdd.pickDifferentAccounts'
  }
  return null
}
