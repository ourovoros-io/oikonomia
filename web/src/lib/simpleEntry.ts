import type { SimpleEntryInput } from './api'

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

export function pickDefault(
  accounts: AccountLike[],
  type: AccountLike['account_type'],
  nameHints: string[] = [],
): string {
  const active = accounts.filter((a) => a.is_active && a.account_type === type)
  for (const hint of nameHints) {
    const found = active.find((a) => a.name.toLowerCase().includes(hint.toLowerCase()))
    if (found) return found.id
  }
  return active[0]?.id ?? ''
}

export function accountsOf(
  accounts: AccountLike[],
  types: AccountLike['account_type'][],
): AccountLike[] {
  return accounts.filter((a) => a.is_active && types.includes(a.account_type))
}

export function kindDefaultAccounts(kind: EntryKind, list: AccountLike[]) {
  if (kind === 'expense') {
    return {
      categoryId: pickDefault(list, 'expense', ['food', 'utilities', 'bills', 'other']),
      walletId: pickDefault(list, 'asset', ['checking', 'bank', 'cash']),
      payableId: '',
      fromId: '',
      toId: '',
    }
  }
  if (kind === 'income') {
    return {
      categoryId: pickDefault(list, 'income', ['salary', 'sales', 'freelance']),
      walletId: pickDefault(list, 'asset', ['checking', 'bank', 'cash']),
      payableId: '',
      fromId: '',
      toId: '',
    }
  }
  if (kind === 'bill') {
    return {
      categoryId: pickDefault(list, 'expense', [
        'utilities',
        'bills',
        'housing',
        'subscription',
        'rent',
      ]),
      walletId: pickDefault(list, 'asset', ['checking', 'bank', 'cash']),
      payableId: pickDefault(list, 'liability', [
        'bills payable',
        'accounts payable',
        'payable',
      ]),
      fromId: '',
      toId: '',
    }
  }
  const fromId = pickDefault(list, 'asset', ['checking', 'bank'])
  const savings = pickDefault(list, 'asset', ['savings', 'cash'])
  return {
    categoryId: '',
    walletId: '',
    payableId: '',
    fromId,
    toId: savings || pickDefault(list, 'asset', []),
  }
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
    bill_status: args.kind === 'bill' ? args.billStatus : null,
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
  if ((kind === 'expense' || kind === 'income') && (!categoryId || !walletId)) {
    return 'Pick accounts'
  }
  if (kind === 'bill') {
    if (!categoryId) return 'Pick category'
    if (billStatus === 'paid' && !walletId) return 'Pick wallet'
    if (billStatus === 'unpaid' && !payableId) return 'Pick payable'
  }
  if (kind === 'transfer') {
    if (!fromId || !toId || fromId === toId) return 'Pick different accounts'
  }
  return null
}
