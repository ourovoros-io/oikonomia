import { useEffect, useState } from 'react'
import { Download, Loader2 } from 'lucide-react'
import { api, type DocumentMeta } from '../lib/api'
import { commandErrorMessage } from '../lib/commandError'
import { formatBytes } from '../lib/files'
import { Modal } from './Modal'
import { Button } from './ui'
import { useI18n } from '../lib/I18nProvider'

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64)
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i += 1) bytes[i] = bin.charCodeAt(i)
  return bytes
}

type Props = {
  /** Non-null id opens the viewer for that document. */
  documentId: string | null
  onClose: () => void
  onError: (message: string) => void
  /** The last failure of an action in this dialog, drawn at the top of it. */
  error?: string | null
}

/**
 * In-memory document viewer: decrypted bytes live only in a blob URL that is
 * revoked when the viewer closes. "Save a copy" is the explicit export path.
 */
export function DocumentViewerModal({ documentId, onClose, onError, error = null }: Props) {
  const { t } = useI18n()
  const [meta, setMeta] = useState<DocumentMeta | null>(null)
  const [bytes, setBytes] = useState<Uint8Array | null>(null)
  const [exporting, setExporting] = useState(false)

  useEffect(() => {
    if (!documentId) {
      setMeta(null)
      setBytes(null)
      return
    }
    let cancelled = false
    void api
      .documentGet(documentId)
      .then((doc) => {
        if (cancelled) return
        setMeta(doc.meta)
        setBytes(base64ToBytes(doc.data_base64))
      })
      .catch((err) => {
        if (cancelled) return
        onError(commandErrorMessage(err, 'viewer.openFailed'))
        onClose()
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [documentId])

  const [blobUrl, setBlobUrl] = useState<string | null>(null)

  // Object URLs are real resources: create and revoke in one effect so
  // StrictMode's double-invoked render helpers cannot leak a registration.
  useEffect(() => {
    if (!bytes || !meta) {
      setBlobUrl(null)
      return
    }
    const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: meta.mime_type }))
    setBlobUrl(url)
    return () => URL.revokeObjectURL(url)
  }, [bytes, meta])

  async function onExport() {
    if (!documentId) return
    setExporting(true)
    try {
      await api.documentExport(documentId)
    } catch (err) {
      onError(commandErrorMessage(err, 'viewer.exportFailed'))
    } finally {
      setExporting(false)
    }
  }

  if (!documentId) return null

  const isImage = meta?.mime_type.startsWith('image/') ?? false
  const isPdf = meta?.mime_type === 'application/pdf'
  const isText = meta?.mime_type === 'text/plain'

  return (
    <Modal
      open
      title={meta?.filename ?? t('viewer.document')}
      description={meta ? `${meta.mime_type} · ${formatBytes(meta.size_bytes)}` : undefined}
      maxWidth="max-w-4xl"
      error={error}
      onClose={onClose}
    >
      {!meta || !bytes ? (
        <div className="flex h-40 items-center justify-center text-[var(--color-muted)]">
          <Loader2 className="size-5 animate-spin" />
        </div>
      ) : (
        <div className="space-y-4">
          {isImage && blobUrl ? (
            <img
              src={blobUrl}
              alt={meta.filename}
              className="mx-auto max-h-[65vh] w-auto max-w-full rounded-xl border border-[var(--color-border)]"
            />
          ) : null}

          {isPdf && blobUrl ? (
            <iframe
              src={blobUrl}
              title={meta.filename}
              className="h-[65vh] w-full rounded-xl border border-[var(--color-border)] bg-white"
            />
          ) : null}

          {isText ? (
            <pre className="max-h-[65vh] overflow-auto rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4 text-xs whitespace-pre-wrap text-[var(--color-fg-secondary)]">
              {new TextDecoder().decode(bytes)}
            </pre>
          ) : null}

          {!isImage && !isPdf && !isText ? (
            <p className="py-8 text-center text-sm text-[var(--color-muted)]">
              {t('viewer.noPreview', { mime: meta.mime_type })}
            </p>
          ) : null}

          <div className="flex justify-end border-t border-[var(--color-border)] pt-4">
            <Button variant="secondary" busy={exporting} onClick={() => void onExport()}>
              <Download className="size-4" />
              {exporting ? t('viewer.saving') : t('viewer.saveCopy')}
            </Button>
          </div>
        </div>
      )}
    </Modal>
  )
}
