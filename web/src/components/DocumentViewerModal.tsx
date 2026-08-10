import { useEffect, useMemo, useState } from 'react'
import { Download, Loader2 } from 'lucide-react'
import { api, type DocumentMeta } from '../lib/api'
import type { CommandError } from '../lib/tauri'
import { formatBytes } from '../lib/files'
import { Modal } from './Modal'
import { Button } from './ui'

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
}

/**
 * In-memory document viewer: decrypted bytes live only in a blob URL that is
 * revoked when the viewer closes. "Save a copy" is the explicit export path.
 */
export function DocumentViewerModal({ documentId, onClose, onError }: Props) {
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
        onError((err as CommandError).message || 'Could not open document')
        onClose()
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [documentId])

  const blobUrl = useMemo(() => {
    if (!bytes || !meta) return null
    return URL.createObjectURL(new Blob([bytes as BlobPart], { type: meta.mime_type }))
  }, [bytes, meta])

  useEffect(() => {
    return () => {
      if (blobUrl) URL.revokeObjectURL(blobUrl)
    }
  }, [blobUrl])

  async function onExport() {
    if (!documentId) return
    setExporting(true)
    try {
      await api.documentExport(documentId)
    } catch (err) {
      onError((err as CommandError).message || 'Could not save a copy')
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
      title={meta?.filename ?? 'Document'}
      description={meta ? `${meta.mime_type} · ${formatBytes(meta.size_bytes)}` : undefined}
      maxWidth="max-w-4xl"
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
              No in-app preview for {meta.mime_type} — use Save a copy to open it elsewhere.
            </p>
          ) : null}

          <div className="flex justify-end border-t border-[var(--color-border)] pt-4">
            <Button variant="secondary" busy={exporting} onClick={() => void onExport()}>
              <Download className="size-4" />
              {exporting ? 'Saving…' : 'Save a copy'}
            </Button>
          </div>
        </div>
      )}
    </Modal>
  )
}
