import { useRef, useState } from 'react'
import { Download, Eye, Paperclip, Pencil, Plus, Trash2 } from 'lucide-react'
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
import { useI18n } from '../lib/I18nProvider'

type Props = {
  /** Non-null view opens the modal. */
  view: PostedEntryView | null
  accounts: Map<string, Account>
  /** Documents linked to this entry. */
  documents: DocumentMeta[]
  currency: string
  onClose: () => void
  /** Open the entry in the edit form. */
  onEdit: () => void
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
  onEdit,
  onView,
  onChanged,
  onError,
}: Props) {
  const { t } = useI18n()
  const [busyId, setBusyId] = useState<string | null>(null)
  const [attachBusy, setAttachBusy] = useState(false)
  const [deleteId, setDeleteId] = useState<string | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  if (!view) return null
  const entry = view.entry

  // One operation at a time: busyId is shared by export and delete, so a
  // second action while either is in flight must be refused, not just
  // silently overwrite the flag and re-enable controls mid-operation.
  const anyBusy = attachBusy || busyId !== null

  async function onAttach(file: File) {
    if (!view) return
    if (anyBusy) return
    // Resource guard only — the backend enforces the same cap.
    if (file.size > 8 * 1024 * 1024) {
      onError(t('entry.fileTooLarge'))
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
      onError((err as CommandError).message || t('entry.attachFailed'))
    } finally {
      setAttachBusy(false)
    }
  }

  async function confirmDelete() {
    if (!deleteId) return
    if (busyId !== null) return
    setBusyId(deleteId)
    try {
      await api.documentDelete(deleteId)
      setDeleteId(null)
      await onChanged()
    } catch (err) {
      onError((err as CommandError).message || t('entry.deleteFailed'))
    } finally {
      setBusyId(null)
    }
  }

  async function onExport(id: string) {
    if (busyId !== null) return
    setBusyId(id)
    try {
      await api.documentExport(id)
    } catch (err) {
      onError((err as CommandError).message || t('entry.exportFailed'))
    } finally {
      setBusyId(null)
    }
  }

  return (
    <Modal
      open
      title={entry.description}
      description={`${formatDate(entry.entry_date)}${entry.reference ? t('entry.ref', { reference: entry.reference }) : ''}`}
      onClose={() => {
        if (!anyBusy) onClose()
      }}
    >
      <ConfirmDialog
        open={deleteId !== null}
        title={t('entry.deleteDocTitle')}
        body={t('entry.deleteDocBody')}
        confirmLabel={t('common.delete')}
        danger
        busy={busyId !== null && busyId === deleteId}
        onCancel={() => {
          if (busyId === null) setDeleteId(null)
        }}
        onConfirm={() => void confirmDelete()}
      />

      <div className="space-y-6">
        <div className="overflow-x-auto rounded-xl border border-[var(--color-border)]">
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-[var(--color-surface-2)] text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
                <th className="px-4 py-2.5 text-left">{t('entry.account')}</th>
                <th className="px-4 py-2.5 text-right">{t('entry.debit')}</th>
                <th className="px-4 py-2.5 text-right">{t('entry.credit')}</th>
                <th className="px-4 py-2.5 text-left">{t('entry.memo')}</th>
              </tr>
            </thead>
            <tbody>
              {view.lines.map((line) => {
                const acc = accounts.get(line.account_id)
                return (
                  <tr key={line.id} className="border-t border-[var(--color-border)]">
                    <td className="px-4 py-2.5 text-[var(--color-fg)]">
                      {acc ? `${acc.code} · ${acc.name}` : t('entry.unknownAccount')}
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
              {t('entry.attachments')}
            </span>
            <Button
              variant="secondary"
              size="sm"
              busy={attachBusy}
              disabled={anyBusy}
              onClick={() => fileRef.current?.click()}
            >
              <Plus className="size-3.5" />
              {t('entry.attachFile')}
            </Button>
            <input
              ref={fileRef}
              type="file"
              accept="image/png,image/jpeg,image/webp,application/pdf,text/plain,.pdf,.png,.jpg,.jpeg,.webp,.txt"
              className="hidden"
              disabled={anyBusy}
              onChange={(e) => {
                const file = e.target.files?.[0]
                if (file) void onAttach(file)
                e.target.value = ''
              }}
            />
          </div>

          {documents.length === 0 ? (
            <p className="rounded-xl border border-dashed border-[var(--color-border-strong)] px-4 py-6 text-center text-xs text-[var(--color-muted)]">
              {t('entry.noFiles')}
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
                    aria-label={t('docs.viewAria')}
                    title={t('docs.view')}
                  >
                    <Eye className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    busy={busyId === doc.id}
                    disabled={anyBusy}
                    onClick={() => void onExport(doc.id)}
                    aria-label={t('docs.saveCopyAria')}
                    title={t('docs.saveCopy')}
                  >
                    <Download className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8"
                    disabled={anyBusy}
                    onClick={() => setDeleteId(doc.id)}
                    aria-label={t('docs.deleteAria')}
                    title={t('common.delete')}
                  >
                    <Trash2 className="size-4" />
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="flex justify-end border-t border-[var(--color-border)] pt-4">
          <Button variant="secondary" disabled={anyBusy} onClick={onEdit}>
            <Pencil className="size-3.5" />
            {t('entry.edit')}
          </Button>
        </div>
      </div>
    </Modal>
  )
}
