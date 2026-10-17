import { ArrowDownLeft, ArrowLeftRight, ArrowUpRight, FileText } from 'lucide-react'
import type { ListedEntryKind } from '../lib/api'
import { isMoneyOut } from '../lib/entryKind'
import { IconBadge } from './ui'

/** The icon of a listed entry, the same in the entry list and on the dashboard. */
export function EntryKindBadge({ kind }: { kind: ListedEntryKind }) {
  const tone = kind === 'income' ? 'money-in' : isMoneyOut(kind) ? 'money-out' : 'muted'
  return (
    <IconBadge tone={tone}>
      {kind === 'income' ? (
        <ArrowDownLeft className="size-4" />
      ) : kind === 'bill' ? (
        <FileText className="size-4" />
      ) : kind === 'expense' ? (
        <ArrowUpRight className="size-4" />
      ) : (
        <ArrowLeftRight className="size-4" />
      )}
    </IconBadge>
  )
}
