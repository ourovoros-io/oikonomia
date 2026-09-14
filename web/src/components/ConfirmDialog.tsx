import { useId, useRef, type ReactNode } from 'react'
import { AlertTriangle, CircleCheck } from 'lucide-react'
import { Button } from './ui'
import { useDialogFocus } from './useDialogFocus'
import { useI18n } from '../lib/I18nProvider'

type ConfirmTone = 'danger' | 'accent' | 'success'

type Props = {
  open: boolean
  title: string
  body: string
  confirmLabel?: string
  cancelLabel?: string
  busyLabel?: string
  /** @deprecated Prefer `tone="danger"`. Kept so existing callers stay valid. */
  danger?: boolean
  tone?: ConfirmTone
  busy?: boolean
  children?: ReactNode
  onConfirm: () => void
  onCancel: () => void
}

function resolveTone(tone: ConfirmTone | undefined, danger: boolean): ConfirmTone {
  if (tone) return tone
  return danger ? 'danger' : 'accent'
}

/** In-app confirm modal — native `window.confirm` is unreliable in Tauri webviews. */
export function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  cancelLabel,
  busyLabel,
  danger = false,
  tone,
  busy = false,
  children,
  onConfirm,
  onCancel,
}: Props) {
  const { t } = useI18n()
  const titleId = useId()
  const panelRef = useRef<HTMLDivElement>(null)
  const resolvedConfirm = confirmLabel ?? t('common.confirm')
  const resolvedCancel = cancelLabel ?? t('common.cancel')
  const resolvedBusy = busyLabel ?? t('common.working')
  const resolvedTone = resolveTone(tone, danger)

  useDialogFocus(panelRef, open, () => {
    if (!busy) onCancel()
  })

  if (!open) return null

  const iconWrap =
    resolvedTone === 'danger'
      ? 'bg-[var(--color-danger-soft)] text-[var(--color-danger)]'
      : resolvedTone === 'success'
        ? 'bg-[var(--color-success-soft)] text-[var(--color-success)]'
        : 'bg-[var(--color-accent-soft)] text-[var(--color-accent)]'

  return (
    <div
      className="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onClick={onCancel}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        className="glass-dialog w-full max-w-md rounded-[26px] p-6 outline-none"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex gap-3">
          <span
            className={`flex size-10 shrink-0 items-center justify-center rounded-[12px] ${iconWrap}`}
          >
            {resolvedTone === 'success' ? (
              <CircleCheck className="size-5" strokeWidth={1.75} />
            ) : (
              <AlertTriangle className="size-5" strokeWidth={1.75} />
            )}
          </span>
          <div className="min-w-0">
            <h2 id={titleId} className="text-base font-semibold text-[var(--color-fg)]">
              {title}
            </h2>
            <p className="mt-1.5 text-sm leading-relaxed text-[var(--color-muted)]">{body}</p>
          </div>
        </div>
        {children ? <div className="mt-4">{children}</div> : null}
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="secondary" onClick={onCancel} disabled={busy}>
            {resolvedCancel}
          </Button>
          <Button
            variant={resolvedTone === 'danger' ? 'danger' : 'primary'}
            onClick={onConfirm}
            busy={busy}
          >
            {busy ? resolvedBusy : resolvedConfirm}
          </Button>
        </div>
      </div>
    </div>
  )
}
