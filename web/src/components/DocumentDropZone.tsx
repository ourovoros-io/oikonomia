import { useCallback, useEffect, useRef, useState, type DragEvent } from 'react'
import { FileUp, Loader2, Sparkles } from 'lucide-react'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { api, type AnalyzerStatus, type DocumentSuggestion } from '../lib/api'
import { isTauri, type CommandError } from '../lib/tauri'
import { cn } from './ui'

type Props = {
  entityId: string
  disabled?: boolean
  onSuggestion: (suggestion: DocumentSuggestion) => void
  onError: (message: string) => void
}

function mimeFromName(name: string, fallback = ''): string {
  const lower = name.toLowerCase()
  if (lower.endsWith('.pdf')) return 'application/pdf'
  if (lower.endsWith('.png')) return 'image/png'
  if (lower.endsWith('.jpg') || lower.endsWith('.jpeg')) return 'image/jpeg'
  if (lower.endsWith('.webp')) return 'image/webp'
  if (lower.endsWith('.txt')) return 'text/plain'
  return fallback || 'application/octet-stream'
}

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      const result = reader.result
      if (typeof result !== 'string') {
        reject(new Error('Could not read file'))
        return
      }
      const comma = result.indexOf(',')
      resolve(comma >= 0 ? result.slice(comma + 1) : result)
    }
    reader.onerror = () => reject(new Error('Could not read file'))
    reader.readAsDataURL(file)
  })
}

export function DocumentDropZone({ entityId, disabled, onSuggestion, onError }: Props) {
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
          hint: 'Built-in analyzer status unavailable.',
        }),
      )
  }, [])

  const processFile = useCallback(
    async (file: File) => {
      if (disabled || busyRef.current) return
      // Resource guard only — the backend enforces the same cap (MAX_DOCUMENT_BYTES);
      // checking here avoids reading a huge file into memory and across IPC first.
      if (file.size > 8 * 1024 * 1024) {
        const msg = 'File too large (max 8 MB)'
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
        onSuggestion(suggestion)
      } catch (err) {
        const msg = (err as CommandError).message || 'Could not analyze document'
        setLocalError(msg)
        onError(msg)
      } finally {
        busyRef.current = false
        setBusy(false)
        setDragOver(false)
      }
    },
    [disabled, entityId, onError, onSuggestion],
  )

  const processPath = useCallback(
    async (path: string) => {
      if (disabled || busyRef.current) return
      busyRef.current = true
      setBusy(true)
      setLocalError(null)
      try {
        const suggestion = await api.documentAnalyzePath({ entityId, path })
        onSuggestion(suggestion)
      } catch (err) {
        const msg = (err as CommandError).message || 'Could not analyze document'
        setLocalError(msg)
        onError(msg)
      } finally {
        busyRef.current = false
        setBusy(false)
        setDragOver(false)
      }
    },
    [disabled, entityId, onError, onSuggestion],
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
              setLocalError('No file path received from drop — try clicking to choose a file.')
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
      setLocalError('No file received. Try clicking to choose a file.')
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
              'radial-gradient(900px 280px at 50% -20%, rgba(139,92,246,0.18), transparent 55%)',
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
          {busy ? 'Analyzing document…' : 'Drop a bill, invoice, receipt, or bank statement here'}
        </p>
        <p className="pointer-events-none mx-auto mt-1.5 max-w-md text-xs leading-relaxed text-[var(--color-muted)]">
          PDF (invoices, bills, bank statements), photos of receipts (PNG/JPEG/WebP), or text. Fully
          offline — nothing leaves this device. Click to choose a file.
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
