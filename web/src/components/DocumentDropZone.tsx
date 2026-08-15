import { useCallback, useEffect, useRef, useState, type DragEvent } from 'react'
import { FileUp, Loader2, Sparkles } from 'lucide-react'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { api, type AnalyzerStatus, type DocumentSuggestion, type PendingDocSource } from '../lib/api'
import { isTauri, type CommandError } from '../lib/tauri'
import { fileToBase64, mimeFromName } from '../lib/files'
import { cn } from '../lib/cn'
import { useI18n } from '../lib/I18nProvider'

type Props = {
  entityId: string
  disabled?: boolean
  onSuggestion: (suggestion: DocumentSuggestion, source: PendingDocSource) => void
  onError: (message: string) => void
}

export function DocumentDropZone({ entityId, disabled, onSuggestion, onError }: Props) {
  const { t } = useI18n()
  const [dragOver, setDragOver] = useState(false)
  const [busy, setBusy] = useState(false)
  const [localError, setLocalError] = useState<string | null>(null)
  const [status, setStatus] = useState<AnalyzerStatus | null>(null)
  const busyRef = useRef(false)

  useEffect(() => {
    void api
      .documentAnalyzerStatus()
      .then(setStatus)
      .catch(() =>
        setStatus({
          ocr_available: false,
          offline: true,
          hint: t('drop.statusUnavailable'),
        }),
      )
  }, [])

  const processFile = useCallback(
    async (file: File) => {
      if (disabled || busyRef.current) return
      // Resource guard only — the backend enforces the same cap (MAX_DOCUMENT_BYTES);
      // checking here avoids reading a huge file into memory and across IPC first.
      if (file.size > 8 * 1024 * 1024) {
        const msg = t('drop.fileTooLarge')
        setLocalError(msg)
        onError(msg)
        return
      }
      busyRef.current = true
      setBusy(true)
      setLocalError(null)
      try {
        const dataBase64 = await fileToBase64(file)
        const mimeType = file.type || mimeFromName(file.name)
        const suggestion = await api.documentAnalyze({
          entityId,
          filename: file.name,
          mimeType,
          dataBase64,
        })
        onSuggestion(suggestion, { kind: 'file', file })
      } catch (err) {
        const msg = (err as CommandError).message || t('drop.analyzeFailed')
        setLocalError(msg)
        onError(msg)
      } finally {
        busyRef.current = false
        setBusy(false)
        setDragOver(false)
      }
    },
    [disabled, entityId, onError, onSuggestion, t],
  )

  const processPath = useCallback(
    async (path: string) => {
      if (disabled || busyRef.current) return
      busyRef.current = true
      setBusy(true)
      setLocalError(null)
      try {
        const suggestion = await api.documentAnalyzePath({ entityId, path })
        onSuggestion(suggestion, { kind: 'path', path })
      } catch (err) {
        const msg = (err as CommandError).message || t('drop.analyzeFailed')
        setLocalError(msg)
        onError(msg)
      } finally {
        busyRef.current = false
        setBusy(false)
        setDragOver(false)
      }
    },
    [disabled, entityId, onError, onSuggestion, t],
  )

  // Tauri webviews often give empty dataTransfer.files on OS file drops.
  // Use the native drag-drop event which provides filesystem paths instead.
  useEffect(() => {
    if (!isTauri() || disabled) return

    let unlisten: (() => void) | undefined
    let cancelled = false

    void (async () => {
      try {
        unlisten = await getCurrentWebview().onDragDropEvent((event) => {
          if (cancelled) return
          const payload = event.payload
          if (payload.type === 'enter' || payload.type === 'over') {
            setDragOver(true)
            return
          }
          if (payload.type === 'leave') {
            setDragOver(false)
            return
          }
          if (payload.type === 'drop') {
            setDragOver(false)
            const path = payload.paths[0]
            if (path) {
              void processPath(path)
            } else {
              setLocalError(t('drop.noPath'))
            }
          }
        })
      } catch {
        // Older / non-webview environments: HTML5 handlers remain as fallback.
      }
    })()

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [disabled, processPath])

  function onHtmlDrop(e: DragEvent) {
    e.preventDefault()
    e.stopPropagation()
    setDragOver(false)
    // Prefer Tauri path handler; HTML5 files may still work in browser/dev.
    const file = e.dataTransfer.files?.[0]
    if (file && file.size > 0) {
      void processFile(file)
      return
    }
    // In Tauri, paths often only arrive via onDragDropEvent (handled above).
    if (!isTauri()) {
      setLocalError(t('drop.noFile'))
    }
  }

  return (
    <div
      onDragOver={(e) => {
        e.preventDefault()
        e.stopPropagation()
        if (!disabled) setDragOver(true)
      }}
      onDragEnter={(e) => {
        e.preventDefault()
        e.stopPropagation()
        if (!disabled) setDragOver(true)
      }}
      onDragLeave={(e) => {
        e.preventDefault()
        // Only clear when leaving the zone itself
        if (e.currentTarget === e.target) setDragOver(false)
      }}
      onDrop={onHtmlDrop}
      className={cn(
        'relative overflow-hidden rounded-2xl border border-dashed px-4 py-10 text-center transition',
        dragOver
          ? 'border-[var(--color-accent)] bg-[var(--color-accent-soft)]'
          : 'border-[var(--color-border-strong)] bg-[var(--color-surface)]',
        disabled || busy ? 'opacity-60' : 'hover:border-[var(--color-muted)]',
      )}
    >
      {!dragOver ? (
        <div
          className="pointer-events-none absolute inset-0 opacity-80"
          style={{
            background:
              'radial-gradient(900px 280px at 50% -20%, rgba(53,176,107,0.18), transparent 55%)',
          }}
          aria-hidden
        />
      ) : null}
      <input
        type="file"
        accept="image/png,image/jpeg,image/webp,application/pdf,text/plain,.pdf,.png,.jpg,.jpeg,.webp,.txt"
        className="absolute inset-0 z-10 cursor-pointer opacity-0"
        disabled={disabled || busy}
        onChange={(e) => {
          const file = e.target.files?.[0]
          if (file) void processFile(file)
          e.target.value = ''
        }}
      />
      <div className="relative">
        <span className="pointer-events-none mb-3 inline-flex size-12 items-center justify-center rounded-xl bg-[var(--color-accent-soft)] text-[var(--color-accent)]">
          {busy ? (
            <Loader2 className="size-5 animate-spin" />
          ) : (
            <FileUp className="size-5" strokeWidth={1.75} />
          )}
        </span>
        <p className="pointer-events-none text-sm font-semibold text-[var(--color-fg)]">
          {busy ? t('drop.analyzing') : t('drop.title')}
        </p>
        <p className="pointer-events-none mx-auto mt-1.5 max-w-md text-xs leading-relaxed text-[var(--color-muted)]">
          {t('drop.body')}
        </p>
        {status ? (
          <p className="pointer-events-none mx-auto mt-4 inline-flex max-w-lg items-start gap-1.5 text-left text-[11px] leading-snug text-[var(--color-muted)]">
            <Sparkles className="mt-0.5 size-3 shrink-0 text-[var(--color-accent)]" />
            {status.hint}
          </p>
        ) : null}
        {localError ? (
          <p className="pointer-events-none mx-auto mt-3 max-w-lg text-xs text-[var(--color-danger)]">
            {localError}
          </p>
        ) : null}
      </div>
    </div>
  )
}
