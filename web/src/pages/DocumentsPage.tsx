import { useEffect, useMemo, useState } from 'react'
import { Download, Eye, FolderOpen, Trash2 } from 'lucide-react'
import { api, type DocumentMeta, type Entity, type PostedEntryView } from '../lib/api'
import type { CommandError } from '../lib/tauri'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DocumentViewerModal } from '../components/DocumentViewerModal'
import { formatBytes } from '../lib/files'
import { Button, EmptyState, ErrorBanner, PageHeader, Panel } from '../components/ui'

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
  const [busyId, setBusyId] = useState<string | null>(null)

  const entryById = useMemo(() => new Map(entries.map((e) => [e.entry.id, e])), [entries])

  const anyBusy = busyId !== null || deleteBusy

  async function reload() {
    if (!entity) return
    const [d, e] = await Promise.all([api.documentList(entity.id), api.entryList(entity.id)])
    setDocs(d)
    setEntries(e)
  }

  useEffect(() => {
    setViewerDocId(null)
    setDeleteId(null)
    if (!entity) {
      setDocs([])
      setEntries([])
      return
    }
    void reload().catch((err) => setError((err as CommandError).message))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity?.id])

  async function confirmDelete() {
    if (!deleteId || busyId !== null) return
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

  async function onExport(id: string) {
    if (busyId !== null) return
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
            {docs.map((doc) => (
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

                <span className="max-w-48 truncate rounded-full bg-[var(--color-accent-soft)] px-2.5 py-1 text-xs text-[var(--color-accent)]">
                  {entryById.get(doc.entry_id)?.entry.description ?? 'Linked entry'}
                </span>

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
                  disabled={anyBusy}
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
                  disabled={anyBusy}
                  onClick={() => setDeleteId(doc.id)}
                  aria-label="Delete document"
                  title="Delete"
                >
                  <Trash2 className="size-4" />
                </Button>
              </li>
            ))}
          </ul>
        </Panel>
      )}
    </div>
  )
}
