import { useI18n } from '../lib/I18nProvider'
import { cn } from '../lib/cn'

/** Quiet list-row chip. Badge copy only — never the long export aria. */
export function HiddenBadge({ className = '' }: { className?: string }) {
  const { t } = useI18n()
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center bg-[var(--color-surface-elevated)] px-1.5 py-px text-[10px] font-medium leading-4 text-[var(--color-muted)]',
        className,
      )}
    >
      {t('tx.hidden.badge')}
    </span>
  )
}

/** Per-entry Hide checkbox. Label/hint are Writer form keys, not Due/Paid. */
export function HideFromExportControl({
  checked,
  onChange,
  disabled,
  compact = false,
}: {
  checked: boolean
  onChange: (hidden: boolean) => void
  disabled?: boolean
  /** Quick Add save row: label + helper on one line. */
  compact?: boolean
}) {
  const { t } = useI18n()
  const label = t('tx.form.hidden.label')
  const hint = t('tx.form.hidden.hint')

  return (
    <label
      className={cn(
        'flex min-w-0',
        compact ? 'h-6 items-center gap-1.5' : 'items-start gap-2',
      )}
    >
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
        className={cn(
          'shrink-0 accent-[var(--color-accent)]',
          compact ? 'size-3.5' : 'mt-0.5 size-4',
        )}
      />
      {compact ? (
        <span className="flex min-w-0 items-baseline gap-1.5">
          <span className="text-[11px] leading-none text-[var(--color-fg)]">{label}</span>
          <span className="truncate text-[9px] leading-none text-[var(--color-muted)]">{hint}</span>
        </span>
      ) : (
        <span className="min-w-0">
          <span className="block text-sm text-[var(--color-fg)]">{label}</span>
          <span className="block text-xs text-[var(--color-muted)]">{hint}</span>
        </span>
      )}
    </label>
  )
}
