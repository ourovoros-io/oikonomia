import { useEffect, useState } from 'react'
import { Download, Eye, FileImage, FileText, FolderOpen, Trash2 } from 'lucide-react'
import { api, type DocumentMeta, type Entity } from '../lib/api'
import { commandErrorMessage } from '../lib/commandError'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DocumentViewerModal } from '../components/DocumentViewerModal'
import { formatBytes } from '../lib/files'
import { TopBar } from '../components/TopBar'
import { Button, EmptyState, ErrorBanner, IconBadge, Panel } from '../components/ui'
import { useI18n } from '../lib/I18nProvider'

type Props = { entity: Entity | null; onCreateBook?: () => void }

/** created_at is the app-wide "unix:<seconds>" ordering key; render as dd/mm/yyyy. */
function formatCreatedAt(createdAt: string): string {
  const secs = Number(createdAt.replace(/^unix:/, ''))
  if (!Number.isFinite(secs) || secs <= 0) return ''
  return new Date(secs * 1000).toLocaleDateString('el-GR', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
  })
}

/** Every file in the book's vault — always linked to the entry it was saved with. */
export function DocumentsPage({ entity, onCreateBook }: Props) {
  const { t } = useI18n()
  const [docs, setDocs] = useState<DocumentMeta[]>([])
  const [error, setError] = useState<string | null>(null)
  const [viewerDocId, setViewerDocId] = useState<string | null>(null)
  const [deleteId, setDeleteId] = useState<string | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const [busyId, setBusyId] = useState<string | null>(null)

  const anyBusy = busyId !== null || deleteBusy

  async function reload() {
    if (!entity) return
    setDocs(await api.documentList(entity.id))
  }

  useEffect(() => {
    setViewerDocId(null)
    setDeleteId(null)
    if (!entity) {
      setDocs([])
      return
    }
    void reload().catch((err) => setError(commandErrorMessage(err)))
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
      setError(commandErrorMessage(err, 'docs.deleteFailed'))
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
      setError(commandErrorMessage(err, 'docs.exportFailed'))
    } finally {
      setBusyId(null)
    }
  }

  if (!entity) {
    return (
      <EmptyState
        icon={<FolderOpen className="size-5" />}
        title={t('docs.noBookTitle')}
        body={t('docs.noBookBody')}
        action={
          onCreateBook ? (
            <Button onClick={onCreateBook}>{t('empty.createBook')}</Button>
          ) : undefined
        }
      />
    )
  }

  return (
    <div className="space-y-4">
      <TopBar title={t('docs.title')} subtitle={`${entity.name} · ${entity.base_currency}`} />

      <ErrorBanner message={error} onDismiss={() => setError(null)} />

      <ConfirmDialog
        open={deleteId !== null}
        title={t('docs.deleteTitle')}
        body={t('docs.deleteBody')}
        confirmLabel={t('common.delete')}
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
          title={t('docs.emptyTitle')}
          body={t('docs.emptyBody')}
        />
      ) : (
        <Panel
          title={t('docs.vaultFiles')}
          description={t('docs.vaultFilesDesc', { count: docs.length })}
          icon={<FolderOpen className="size-4" />}
        >
          <ul className="divide-y divide-[var(--color-border)]">
            {docs.map((doc) => (
              <li key={doc.id} className="flex items-center gap-4 px-5 py-3">
                <IconBadge tone="info">
                  {doc.mime_type.startsWith('image/') ? (
                    <FileImage className="size-4" strokeWidth={1.75} />
                  ) : (
                    <FileText className="size-4" strokeWidth={1.75} />
                  )}
                </IconBadge>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium text-[var(--color-fg)]">
                    {doc.filename}
                  </div>
                  <div className="truncate text-xs text-[var(--color-muted)] tabular-nums">
                    {formatCreatedAt(doc.created_at) ? (
                      <>
                        {formatCreatedAt(doc.created_at)}
                        <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      </>
                    ) : null}
                    {formatBytes(doc.size_bytes)}
                    <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                    {doc.mime_type}
                  </div>
                </div>

                <span className="max-w-48 truncate rounded-full bg-[var(--color-accent-soft)] px-2.5 py-1 text-xs text-[var(--color-accent)]">
                  {doc.entry_description || t('docs.linkedEntry')}
                </span>

                <Button
                  variant="ghost"
                  size="iconSm"
                  onClick={() => setViewerDocId(doc.id)}
                  aria-label={t('docs.viewAria')}
                  title={t('docs.view')}
                >
                  <Eye className="size-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="iconSm"
                  disabled={anyBusy}
                  busy={busyId === doc.id}
                  onClick={() => void onExport(doc.id)}
                  aria-label={t('docs.saveCopyAria')}
                  title={t('docs.saveCopy')}
                >
                  <Download className="size-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="iconSm"
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
        </Panel>
      )}
    </div>
  )
}
