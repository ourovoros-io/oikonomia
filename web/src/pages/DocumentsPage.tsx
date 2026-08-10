import { useEffect, useMemo, useState } from 'react'
import { Download, Eye, FolderOpen, Link2, Trash2 } from 'lucide-react'
import {
  api,
  formatDate,
  type DocumentMeta,
  type Entity,
  type PostedEntryView,
} from '../lib/api'
import type { CommandError } from '../lib/tauri'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DocumentViewerModal } from '../components/DocumentViewerModal'
import { Modal } from '../components/Modal'
import { formatBytes } from '../lib/files'
import {
  Button,
  EmptyState,
  ErrorBanner,
  Input,
  PageHeader,
  Panel,
} from '../components/ui'

type Props = { entity: Entity | null }

/** created_at is the app-wide "unix:<seconds>" ordering key; render as a local date. */
function formatCreatedAt(createdAt: string): string {
  const secs = Number(createdAt.replace(/^unix:/, ''))
  if (!Number.isFinite(secs) || secs <= 0) return ''
  return new Date(secs * 1000).toLocaleDateString()
}

/** Every file in the entity's vault, including orphans never linked to an entry. */
export function DocumentsPage({ entity }: Props) {
  const [docs, setDocs] = useState<DocumentMeta[]>([])
  const [entries, setEntries] = useState<PostedEntryView[]>([])
  const [error, setError] = useState<string | null>(null)
  const [viewerDocId, setViewerDocId] = useState<string | null>(null)
  const [deleteId, setDeleteId] = useState<string | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const [linkDocId, setLinkDocId] = useState<string | null>(null)
  const [linkSearch, setLinkSearch] = useState('')
  const [busyId, setBusyId] = useState<string | null>(null)

  const entryById = useMemo(() => new Map(entries.map((e) => [e.entry.id, e])), [entries])

  const linkCandidates = useMemo(() => {
    const q = linkSearch.trim().toLowerCase()
    const pool = entries.filter((e) => !e.is_voided)
    if (!q) return pool.slice(0, 25)
    return pool
      .filter(
        (e) =>
          e.entry.description.toLowerCase().includes(q) ||
          e.entry.entry_date.includes(q),
      )
      .slice(0, 25)
  }, [entries, linkSearch])

  async function reload() {
    if (!entity) return
    const [d, e] = await Promise.all([api.documentList(entity.id), api.entryList(entity.id)])
    setDocs(d)
    setEntries(e)
  }

  useEffect(() => {
    if (!entity) {
      setDocs([])
      setEntries([])
      return
    }
    void reload().catch((err) => setError((err as CommandError).message))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity?.id])

  async function confirmDelete() {
    if (!deleteId) return
    setDeleteBusy(true)
    setError(null)
    try {
      await api.documentDelete(deleteId)
      setDeleteId(null)
      await reload()
    } catch (err) {
      setError((err as CommandError).message || 'Could not delete document')
    } finally {
      setDeleteBusy(false)
    }
  }

  async function linkTo(entryId: string) {
    if (!linkDocId) return
    setBusyId(linkDocId)
    setError(null)
    try {
      await api.documentLinkEntry(linkDocId, entryId)
      setLinkDocId(null)
      setLinkSearch('')
      await reload()
    } catch (err) {
      setError((err as CommandError).message || 'Could not link document')
    } finally {
      setBusyId(null)
    }
  }

  async function onExport(id: string) {
    setBusyId(id)
    try {
      await api.documentExport(id)
    } catch (err) {
      setError((err as CommandError).message || 'Could not save a copy')
    } finally {
      setBusyId(null)
    }
  }

  if (!entity) {
    return (
      <EmptyState
        icon={<FolderOpen className="size-5" />}
        title="No book selected"
        body="Create or select a book first."
      />
    )
  }

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Vault"
        title="Documents"
        description="Every file stored in this book's encrypted vault."
        meta="Encrypted at rest"
      />

      <ErrorBanner message={error} />

      <ConfirmDialog
        open={deleteId !== null}
        title="Delete document?"
        body="This permanently removes the file from your vault. It cannot be undone. Journal entries are not affected."
        confirmLabel="Delete"
        danger
        busy={deleteBusy}
        onCancel={() => {
          if (!deleteBusy) setDeleteId(null)
        }}
        onConfirm={() => void confirmDelete()}
      />

      <Modal
        open={linkDocId !== null}
        title="Link to entry"
        description="Pick the journal entry this file belongs to"
        maxWidth="max-w-xl"
        onClose={() => {
          setLinkDocId(null)
          setLinkSearch('')
        }}
      >
        <div className="space-y-4">
          <Input
            value={linkSearch}
            onChange={(e) => setLinkSearch(e.target.value)}
            placeholder="Search by description or date…"
          />
          {linkCandidates.length === 0 ? (
            <p className="py-6 text-center text-sm text-[var(--color-muted)]">
              No entries match.
            </p>
          ) : (
            <ul className="max-h-80 divide-y divide-[var(--color-border)] overflow-y-auto rounded-xl border border-[var(--color-border)]">
              {linkCandidates.map((v) => (
                <li key={v.entry.id}>
                  <button
                    type="button"
                    className="flex w-full items-center gap-3 px-4 py-2.5 text-left transition hover:bg-[var(--color-surface-2)]/60"
                    onClick={() => void linkTo(v.entry.id)}
                  >
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm text-[var(--color-fg)]">
                        {v.entry.description}
                      </div>
                      <div className="text-xs text-[var(--color-muted)]">
                        {formatDate(v.entry.entry_date)}
                      </div>
                    </div>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </Modal>

      <DocumentViewerModal
        documentId={viewerDocId}
        onClose={() => setViewerDocId(null)}
        onError={(msg) => setError(msg)}
      />

      {docs.length === 0 ? (
        <EmptyState
          icon={<FolderOpen className="size-5" />}
          title="No documents yet"
          body="Files you drop on the Transactions page are stored here, encrypted. Attachments to entries also appear in this list."
        />
      ) : (
        <Panel
          title="Vault files"
          description={`${docs.length} stored · encrypted`}
          icon={<FolderOpen className="size-4" />}
        >
          <ul className="divide-y divide-[var(--color-border)]">
            {docs.map((doc) => {
              const linked = doc.entry_id ? entryById.get(doc.entry_id) : undefined
              return (
                <li key={doc.id} className="flex items-center gap-4 px-5 py-3.5">
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-[var(--color-fg)]">
                      {doc.filename}
                    </div>
                    <div className="truncate text-xs text-[var(--color-muted)]">
                      {formatCreatedAt(doc.created_at)}
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      {formatBytes(doc.size_bytes)}
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      {doc.mime_type}
                    </div>
                  </div>

                  {doc.entry_id ? (
                    <span className="max-w-48 truncate rounded-full bg-[var(--color-accent-soft)] px-2.5 py-1 text-xs text-[var(--color-accent)]">
                      {linked?.entry.description ?? 'Linked entry'}
                    </span>
                  ) : (
                    <span className="rounded-full bg-[var(--color-surface-elevated)] px-2.5 py-1 text-xs text-[var(--color-muted)]">
                      Not linked
                    </span>
                  )}

                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    onClick={() => setViewerDocId(doc.id)}
                    aria-label="View document"
                    title="View"
                  >
                    <Eye className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    onClick={() => setLinkDocId(doc.id)}
                    aria-label="Link to entry"
                    title="Link to entry"
                  >
                    <Link2 className="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
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
                    className="h-8 w-8 shrink-0"
                    onClick={() => setDeleteId(doc.id)}
                    aria-label="Delete document"
                    title="Delete"
                  >
                    <Trash2 className="size-4" />
                  </Button>
                </li>
              )
            })}
          </ul>
        </Panel>
      )}
    </div>
  )
}
