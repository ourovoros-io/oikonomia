import type { ListedEntryKind } from './api'

/** A bill takes money out whether it is owed, paid at once, or a payment of one. */
export function isMoneyOut(kind: ListedEntryKind): boolean {
  return kind === 'expense' || kind === 'bill'
}

/** The tone of an entry's amount: in for an income, out for money spent or owed. */
export function amountTone(kind: ListedEntryKind): 'in' | 'out' | 'neutral' {
  if (kind === 'income') return 'in'
  return isMoneyOut(kind) ? 'out' : 'neutral'
}
