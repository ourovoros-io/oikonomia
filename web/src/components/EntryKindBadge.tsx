import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  FileCheck,
  FileText,
  Flag,
  NotebookPen,
  type LucideIcon,
} from 'lucide-react'
import type { ListedEntryKind } from '../lib/api'
import { isMoneyOut } from '../lib/entryKind'
import { IconBadge } from './ui'

/** One icon per kind, so a new kind does not compile until it has one. */
const KIND_ICONS: Record<ListedEntryKind, LucideIcon> = {
  expense: ArrowUpRight,
  income: ArrowDownLeft,
  bill: FileText,
  transfer: ArrowLeftRight,
  opening_balance: Flag,
  other: NotebookPen,
}

/**
 * The icon of a listed entry, the same in the entry list and on the dashboard.
 * `paid` marks a bill a payment settles.
 */
export function EntryKindBadge({ kind, paid = false }: { kind: ListedEntryKind; paid?: boolean }) {
  const tone = kind === 'income' ? 'money-in' : isMoneyOut(kind) ? 'money-out' : 'muted'
  const Icon = paid ? FileCheck : KIND_ICONS[kind]
  return (
    <IconBadge tone={tone}>
      <Icon className="size-4" />
    </IconBadge>
  )
}
