import { useRef, useState } from 'react'
import { Download, Eye, Paperclip, Plus, X } from 'lucide-react'
import {
  api,
  formatDate,
  formatMoney,
  type Account,
  type DocumentMeta,
  type PostedEntryView,
} from '../lib/api'
import type { CommandError } from '../lib/tauri'
import { fileToBase64, formatBytes, mimeFromName } from '../lib/files'
import { ConfirmDialog } from './ConfirmDialog'
import { Modal } from './Modal'
import { Button } from './ui'

type Props = {
  /** Non-null view opens the modal. */
  view: PostedEntryView | null
  accounts: Map<string, Account>
  /** Documents linked to this entry. */
  documents: DocumentMeta[]
  currency: string
  onClose: () => void
  onView: (documentId: string) => void
  onChanged: () => Promise<void>
  onError: (message: string) => void
}

/** Full journal view: lines with account names plus the attachments list. */
export function EntryDetailModal({
  view,
  accounts,
  documents,
  currency,
  onClose,
  onView,
  onChanged,
  onError,
}: Props) {
  const [busyId, setBusyId] = useState<string | null>(null)
  const [attachBusy, setAttachBusy] = useState(false)
  const [unlinkId, setUnlinkId] = useState<string | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  if (!view) return null
  const entry = view.entry

  async function onAttach(file: File) {
    if (!view) return
    // Resource guard only — the backend enforces the same cap.
    if (file.size > 8 * 1024 * 1024) {
      onError('File too large (max 8 MB)')
      return
    }
    setAttachBusy(true)
    try {
      const dataBase64 = await fileToBase64(file)
      await api.documentAttach({
        entityId: view.entry.entity_id,
        entryId: view.entry.id,
        filename: file.name,
        mimeType: file.type || mimeFromName(file.name),
        dataBase64,
      })
      await onChanged()
    } catch (err) {
      onError((err as CommandError).message || 'Could not attach file')
    } finally {
      setAttachBusy(false)
    }
  }

  async function confirmUnlink() {
    if (!unlinkId) return
    setBusyId(unlinkId)
    try {
      await api.documentUnlink(unlinkId)
      setUnlinkId(null)
      await onChanged()
    } catch (err) {
      onError((err as CommandError).message || 'Could not remove attachment')
    } finally {
      setBusyId(null)
    }
  }

  async function onExport(id: string) {
    setBusyId(id)
    try {
      await api.documentExport(id)
    } catch (err) {
      onError((err as CommandError).message || 'Could not save a copy')
    } finally {
      setBusyId(null)
    }
  }

  return (
    <Modal
      open
      title={entry.description}
      description={`${formatDate(entry.entry_date)}${entry.reference ? ` · Ref ${entry.reference}` : ''}`}
      onClose={onClose}
    >
      <ConfirmDialog
        open={unlinkId !== null}
        title="Remove attachment?"
        body="The file stays in your vault under Documents as unlinked — nothing is deleted."
        confirmLabel="Remove"
        busy={busyId !== null && busyId === unlinkId}
        onCancel={() => setUnlinkId(null)}
        onConfirm={() => void confirmUnlink()}
      />

      <div className="space-y-6">
        <div className="overflow-hidden rounded-xl border border-[var(--color-border)]">
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-[var(--color-surface-2)] text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
                <th className="px-4 py-2.5 text-left">Account</th>
                <th className="px-4 py-2.5 text-right">Debit</th>
                <th className="px-4 py-2.5 text-right">Credit</th>
                <th className="px-4 py-2.5 text-left">Memo</th>
              </tr>
            </thead>
            <tbody>
              {view.lines.map((line) => {
                const acc = accounts.get(line.account_id)
                return (
                  <tr key={line.id} className="border-t border-[var(--color-border)]">
                    <td className="px-4 py-2.5 text-[var(--color-fg)]">
                      {acc ? `${acc.code} · ${acc.name}` : 'Unknown account'}
                    </td>
                    <td className="px-4 py-2.5 text-right tabular-nums">
                      {line.debit.amount_minor > 0
                        ? formatMoney(line.debit.amount_minor, currency)
                        : ''}
                    </td>
                    <td className="px-4 py-2.5 text-right tabular-nums">
                      {line.credit.amount_minor > 0
                        ? formatMoney(line.credit.amount_minor, currency)
                        : ''}
                    </td>
                    <td className="px-4 py-2.5 text-[var(--color-muted)]">{line.memo ?? ''}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>

        <div>
          <div className="mb-2 flex items-center justify-between">
            <span className="text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
              Attachments
            </span>
            <Button
              variant="secondary"
              size="sm"
              busy={attachBusy}
              onClick={() => fileRef.current?.click()}
            >
              <Plus className="size-3.5" />
              Attach file
            </Button>
            <input
              ref={fileRef}
              type="file"
              accept="image/png,image/jpeg,image/webp,application/pdf,text/plain,.pdf,.png,.jpg,.jpeg,.webp,.txt"
              className="hidden"
              onChange={(e) => {
                const file = e.target.files?.[0]
                if (file) void onAttach(file)
                e.target.value = ''
              }}
            />
          </div>

          {documents.length === 0 ? (
            <p className="rounded-xl border border-dashed border-[var(--color-border-strong)] px-4 py-6 text-center text-xs text-[var(--color-muted)]">
              No files attached to this entry.
            </p>
          ) : (
            <ul className="divide-y divide-[var(--color-border)] rounded-xl border border-[var(--color-border)]">
              {documents.map((doc) => (
                <li key={doc.id} className="flex items-center gap-3 px-4 py-2.5">
                  <Paperclip className="size-4 shrink-0 text-[var(--color-muted)]" />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm text-[var(--color-fg)]">{doc.filename}</div>
                    <div className="text-xs text-[var(--color-muted)]">
                      {formatBytes(doc.size_bytes)}
                    </div>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    onClick={() => onView(doc.id)}
                    aria-label="View document"
                    title="View"
                  >
                    <Eye className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    busy={busyId === doc.id}
                    onClick={() => void onExport(doc.id)}
                    aria-label="Save a copy"
                    title="Save a copy"
                  >
                    <Download className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    onClick={() => setUnlinkId(doc.id)}
                    aria-label="Remove attachment"
                    title="Remove"
                  >
                    <X className="size-4" />
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </Modal>
  )
}
