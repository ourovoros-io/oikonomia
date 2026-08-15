import { useRef } from 'react'
import { AlertTriangle } from 'lucide-react'
import { Button } from './ui'
import { useDialogFocus } from './useDialogFocus'
import { useI18n } from '../lib/I18nProvider'

type Props = {
  open: boolean
  title: string
  body: string
  confirmLabel?: string
  danger?: boolean
  busy?: boolean
  onConfirm: () => void
  onCancel: () => void
}

/** In-app confirm modal — native `window.confirm` is unreliable in Tauri webviews. */
export function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  danger = false,
  busy = false,
  onConfirm,
  onCancel,
}: Props) {
  const { t } = useI18n()
  const panelRef = useRef<HTMLDivElement>(null)
  const resolvedConfirm = confirmLabel ?? t('common.confirm')

  useDialogFocus(panelRef, open, () => {
    if (!busy) onCancel()
  })

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="confirm-title"
      onClick={onCancel}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        className="w-full max-w-md rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] p-6 shadow-2xl outline-none"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex gap-3">
          <span
            className={
              danger
                ? 'flex size-10 shrink-0 items-center justify-center rounded-lg bg-[var(--color-danger-soft)] text-[var(--color-danger)]'
                : 'flex size-10 shrink-0 items-center justify-center rounded-lg bg-[var(--color-accent-soft)] text-[var(--color-accent)]'
            }
          >
            <AlertTriangle className="size-5" strokeWidth={1.75} />
          </span>
          <div className="min-w-0">
            <h2 id="confirm-title" className="text-base font-semibold text-[var(--color-fg)]">
              {title}
            </h2>
            <p className="mt-1.5 text-sm leading-relaxed text-[var(--color-muted)]">{body}</p>
          </div>
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="secondary" onClick={onCancel} disabled={busy}>
            {t('common.cancel')}
          </Button>
          <Button variant={danger ? 'danger' : 'primary'} onClick={onConfirm} busy={busy}>
            {busy ? t('common.working') : resolvedConfirm}
          </Button>
        </div>
      </div>
    </div>
  )
}
