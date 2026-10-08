import { useId, useRef, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
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

  // backdrop-filter makes an element the containing block for position:fixed
  // descendants in WebKit, so a Modal rendered inside another glass surface
  // (a pane, or another dialog) would otherwise be sized to and clipped by
  // it instead of the viewport. Porting to document.body sidesteps that.
  //
  // Anchored near the top, not centred: a centred dialog re-centres whenever
  // its content changes height (an error appears, the entry type changes), so
  // the controls move under the pointer. Anchored, only the bottom edge moves.
  return createPortal(
    <div
      className="glass-scrim fixed inset-0 z-50 flex items-start justify-center p-4 pt-[7vh]"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onClick={onClose}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        className={cn(
          'glass-dialog flex max-h-[calc(93vh-2rem)] w-full flex-col overflow-hidden rounded-[24px] outline-none',
          maxWidth,
        )}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-4 border-b border-[var(--color-border)] px-6 py-5">
          <div className="min-w-0">
            <h2 id={titleId} className="text-xl leading-7 font-semibold text-[var(--color-fg)]">
              {title}
            </h2>
            {description ? (
              <p className="mt-1 text-[13px] leading-5 text-[var(--color-muted)]">{description}</p>
            ) : null}
          </div>
          <Button
            variant="ghost"
            size="iconSm"
            // Centres the 32px button on the 28px title line.
            className="-mt-0.5"
            onClick={onClose}
            aria-label={t('common.close')}
            title={t('common.close')}
          >
            <X className="size-4" />
          </Button>
        </div>

        <div className="overflow-y-auto p-6">{children}</div>
      </div>
    </div>,
    document.body,
  )
}
