import { useId, useRef, type ReactNode } from 'react'
import { X } from 'lucide-react'
import { Button } from './ui'
import { cn } from '../lib/cn'
import { useDialogFocus } from './useDialogFocus'
import { useI18n } from '../lib/I18nProvider'

type Props = {
  open: boolean
  title: string
  description?: string
  /** Tailwind max-width class for the panel. */
  maxWidth?: string
  onClose: () => void
  children: ReactNode
}

/** Overlay dialog: closes on Escape, backdrop click, or the X button. */
export function Modal({
  open,
  title,
  description,
  maxWidth = 'max-w-3xl',
  onClose,
  children,
}: Props) {
  const { t } = useI18n()
  const titleId = useId()
  const panelRef = useRef<HTMLDivElement>(null)

  useDialogFocus(panelRef, open, onClose)

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onClick={onClose}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        className={cn(
          'flex max-h-[88vh] w-full flex-col overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-2xl outline-none',
          maxWidth,
        )}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-3 border-b border-[var(--color-border)] px-6 py-4">
          <div className="min-w-0">
            <h2 id={titleId} className="text-base font-semibold text-[var(--color-fg)]">
              {title}
            </h2>
            {description ? (
              <p className="mt-0.5 text-xs text-[var(--color-muted)]">{description}</p>
            ) : null}
          </div>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 shrink-0"
            onClick={onClose}
            aria-label={t('common.close')}
            title={t('common.close')}
          >
            <X className="size-4" />
          </Button>
        </div>

        <div className="overflow-y-auto p-6">{children}</div>
      </div>
    </div>
  )
}
