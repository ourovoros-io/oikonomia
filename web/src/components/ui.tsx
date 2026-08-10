import { ChevronDown, Loader2 } from 'lucide-react'
import { useState } from 'react'
import type {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
} from 'react'

export function cn(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(' ')
}

const controlBase =
  'h-10 w-full rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] px-3 text-sm text-[var(--color-fg)] outline-none transition placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)] focus:ring-2 focus:ring-[var(--color-accent)]/25 disabled:opacity-50'

const surfaceShadow = 'shadow-[0_1px_0_rgba(255,255,255,0.03)]'

/** Soft colored icon chip used across metric cards and activity rows. */
export function IconBadge({
  children,
  tone = 'accent',
  size = 'md',
  className = '',
}: {
  children: ReactNode
  tone?: 'accent' | 'success' | 'danger' | 'warning' | 'info' | 'muted'
  size?: 'sm' | 'md'
  className?: string
}) {
  const tones = {
    accent: 'bg-[var(--color-accent-soft)] text-[var(--color-accent)]',
    success: 'bg-[var(--color-success-soft)] text-[var(--color-success)]',
    danger: 'bg-[var(--color-danger-soft)] text-[var(--color-danger)]',
    warning: 'bg-[var(--color-warning-soft)] text-[var(--color-warning)]',
    info: 'bg-[var(--color-info-soft)] text-[var(--color-info)]',
    muted: 'bg-[var(--color-surface-elevated)] text-[var(--color-muted)]',
  }
  const sizes = {
    sm: 'size-8',
    md: 'size-9',
  }
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-lg',
        tones[tone],
        sizes[size],
        className,
      )}
    >
      {children}
    </span>
  )
}

/** Primary surface: rounded-2xl panel matching the dashboard language. */
export function Card({
  children,
  className = '',
  padding = 'md',
}: {
  children: ReactNode
  className?: string
  padding?: 'none' | 'sm' | 'md' | 'lg'
}) {
  const pad = {
    none: '',
    sm: 'p-4',
    md: 'p-5',
    lg: 'p-6 sm:p-8',
  }[padding]
  return (
    <div
      className={cn(
        'rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)]',
        surfaceShadow,
        pad,
        className,
      )}
    >
      {children}
    </div>
  )
}

/**
 * Hero surface with soft radial washes (dashboard net panel).
 * Use for key numbers or primary callouts.
 */
export function Hero({
  children,
  className = '',
  accent = 'accent',
}: {
  children: ReactNode
  className?: string
  accent?: 'accent' | 'success' | 'neutral'
}) {
  const wash =
    accent === 'success'
      ? 'radial-gradient(1000px 360px at 12% -10%, rgba(45,212,191,0.18), transparent 55%), radial-gradient(700px 280px at 90% 0%, rgba(53,176,107,0.12), transparent 50%)'
      : accent === 'neutral'
        ? 'radial-gradient(1000px 360px at 10% -10%, rgba(53,176,107,0.16), transparent 55%)'
        : 'radial-gradient(1200px 400px at 10% -10%, rgba(53,176,107,0.3), transparent 55%), radial-gradient(800px 300px at 90% 0%, rgba(56,189,248,0.12), transparent 50%), radial-gradient(600px 260px at 55% 110%, rgba(45,212,191,0.1), transparent 55%)'

  return (
    <div
      className={cn(
        'relative overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)]',
        className,
      )}
    >
      <div
        className="pointer-events-none absolute inset-0 opacity-90"
        style={{ background: wash }}
        aria-hidden
      />
      <div className="relative">{children}</div>
    </div>
  )
}

/** Section panel with title bar — activity lists, report blocks, etc. */
export function Panel({
  title,
  description,
  icon,
  actions,
  children,
  className = '',
}: {
  title: string
  description?: string
  icon?: ReactNode
  actions?: ReactNode
  children: ReactNode
  className?: string
}) {
  return (
    <div
      className={cn(
        'overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)]',
        surfaceShadow,
        className,
      )}
    >
      <div className="flex items-center justify-between gap-3 border-b border-[var(--color-border)] px-5 py-4">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-[var(--color-fg)]">{title}</h3>
          {description ? <p className="text-xs text-[var(--color-muted)]">{description}</p> : null}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {actions}
          {icon ? <span className="text-[var(--color-muted)]">{icon}</span> : null}
        </div>
      </div>
      {children}
    </div>
  )
}

/**
 * Collapsible section: header row always visible with a chevron at the end,
 * body rendered only while expanded. Collapsed by default.
 */
export function CollapsibleSection({
  title,
  description,
  icon,
  tone = 'accent',
  defaultOpen = false,
  flush = false,
  children,
}: {
  title: string
  description?: string
  icon?: ReactNode
  tone?: 'accent' | 'success' | 'danger' | 'warning' | 'info' | 'muted'
  defaultOpen?: boolean
  /** Body without padding, for lists that manage their own edges. */
  flush?: boolean
  children: ReactNode
}) {
  const [open, setOpen] = useState(defaultOpen)

  return (
    <div
      className={cn(
        'overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)]',
        surfaceShadow,
      )}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex w-full items-center gap-3 px-5 py-4 text-left transition hover:bg-[var(--color-surface-2)]/60"
      >
        {icon ? (
          <IconBadge tone={tone} size="sm">
            {icon}
          </IconBadge>
        ) : null}
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-[var(--color-fg)]">{title}</h3>
          {description ? <p className="text-xs text-[var(--color-muted)]">{description}</p> : null}
        </div>
        <ChevronDown
          className={cn(
            'size-4 shrink-0 text-[var(--color-muted)] transition-transform duration-200',
            open && 'rotate-180',
          )}
        />
      </button>

      {open ? (
        <div className={cn('border-t border-[var(--color-border)]', !flush && 'p-5 sm:p-6')}>
          {children}
        </div>
      ) : null}
    </div>
  )
}

export function Button({
  variant = 'primary',
  size = 'md',
  className = '',
  busy = false,
  disabled,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'primary' | 'secondary' | 'danger' | 'ghost'
  size?: 'sm' | 'md' | 'icon'
  /** Shows a spinner and disables the button while a slow action runs. */
  busy?: boolean
}) {
  const styles = {
    primary: 'bg-[var(--color-accent)] text-white hover:bg-[var(--color-accent-hover)] shadow-sm',
    secondary:
      'border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] text-[var(--color-fg-secondary)] hover:border-[var(--color-accent)] hover:text-[var(--color-fg)]',
    danger:
      'border border-transparent bg-[var(--color-danger-soft)] text-[var(--color-danger)] hover:bg-[var(--color-danger)]/20',
    ghost:
      'text-[var(--color-muted)] hover:bg-[var(--color-surface-elevated)] hover:text-[var(--color-fg)]',
  }
  const sizes = {
    sm: 'h-8 gap-1.5 px-2.5 text-xs',
    md: 'h-10 gap-2 px-3.5 text-sm',
    icon: 'h-10 w-10 shrink-0 justify-center p-0',
  }
  return (
    <button
      type="button"
      className={cn(
        'inline-flex items-center justify-center rounded-[var(--radius-control)] font-medium transition disabled:cursor-not-allowed disabled:opacity-50',
        styles[variant],
        sizes[size],
        className,
      )}
      disabled={disabled || busy}
      {...props}
    >
      {busy ? <Loader2 className="size-3.5 shrink-0 animate-spin" /> : null}
      {children}
    </button>
  )
}

export function Input({ className = '', ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={cn(controlBase, className)} {...props} />
}

export function Select({
  className = '',
  children,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <div className={cn('relative', className)}>
      <select className={cn(controlBase, 'ui-select cursor-pointer pr-9')} {...props}>
        {children}
      </select>
      <ChevronDown
        className="pointer-events-none absolute top-1/2 right-2.5 size-4 -translate-y-1/2 text-[var(--color-muted)]"
        strokeWidth={1.75}
        aria-hidden
      />
    </div>
  )
}

export function Label({ children }: { children: ReactNode }) {
  return (
    <span className="mb-1.5 block text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
      {children}
    </span>
  )
}

export function Field({
  label,
  children,
  className = '',
}: {
  label: string
  children: ReactNode
  className?: string
}) {
  return (
    <label className={cn('block min-w-0', className)}>
      <Label>{label}</Label>
      {children}
    </label>
  )
}

/**
 * Page chrome matching the dashboard: uppercase eyebrow, large title,
 * muted description, optional actions / meta.
 */
export function PageHeader({
  eyebrow,
  title,
  description,
  actions,
  meta,
}: {
  eyebrow?: string
  title: string
  description?: string
  actions?: ReactNode
  meta?: ReactNode
}) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div className="min-w-0">
        {eyebrow ? (
          <p className="text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
            {eyebrow}
          </p>
        ) : null}
        <h2
          className={cn(
            'leading-tight font-semibold tracking-tight text-[var(--color-fg)]',
            eyebrow ? 'mt-1 text-[1.75rem]' : 'text-[1.5rem]',
          )}
        >
          {title}
        </h2>
        {description ? (
          <p className="mt-1 text-sm text-[var(--color-muted)]">{description}</p>
        ) : null}
      </div>
      <div className="flex shrink-0 flex-wrap items-center gap-2">
        {meta ? <div className="text-xs text-[var(--color-muted)]">{meta}</div> : null}
        {actions}
      </div>
    </div>
  )
}

export function ErrorBanner({ message }: { message: string | null }) {
  if (!message) return null
  return (
    <div className="mb-4 flex items-start gap-2 rounded-xl border border-[var(--color-danger)]/30 bg-[var(--color-danger-soft)] px-4 py-3 text-sm text-[var(--color-danger)]">
      <span className="min-w-0 flex-1">{message}</span>
    </div>
  )
}

export function EmptyState({
  icon,
  title,
  body,
  action,
}: {
  icon?: ReactNode
  title: string
  body: string
  action?: ReactNode
}) {
  return (
    <div className="flex flex-col items-center rounded-2xl border border-dashed border-[var(--color-border-strong)] bg-[var(--color-surface)]/50 px-6 py-16 text-center">
      {icon ? (
        <div className="mb-4 flex size-12 items-center justify-center rounded-xl bg-[var(--color-surface-elevated)] text-[var(--color-muted)]">
          {icon}
        </div>
      ) : null}
      <p className="text-sm font-semibold text-[var(--color-fg)]">{title}</p>
      <p className="mt-1.5 max-w-sm text-sm text-[var(--color-muted)]">{body}</p>
      {action ? <div className="mt-6">{action}</div> : null}
    </div>
  )
}

export function Table({ children }: { children: ReactNode }) {
  return (
    <div
      className={cn(
        'overflow-x-auto rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)]',
        surfaceShadow,
      )}
    >
      <table className="w-full min-w-[640px] border-collapse text-left text-sm">{children}</table>
    </div>
  )
}

export function Th({ children, className = '' }: { children?: ReactNode; className?: string }) {
  return (
    <th
      className={cn(
        'h-11 border-b border-[var(--color-border)] bg-[var(--color-surface-2)] px-4 text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase first:pl-5 last:pr-5',
        className,
      )}
    >
      {children}
    </th>
  )
}

export function Td({
  children,
  className = '',
  colSpan,
}: {
  children?: ReactNode
  className?: string
  colSpan?: number
}) {
  return (
    <td
      colSpan={colSpan}
      className={cn(
        'h-12 border-b border-[var(--color-border)] px-4 align-middle text-[var(--color-fg-secondary)] transition first:pl-5 last:pr-5 last:border-b-0 group-hover:bg-[var(--color-surface-2)]/40',
        className,
      )}
    >
      {children}
    </td>
  )
}

/** Selectable card for template / type choices (less typing). */
export function ChoiceCard({
  selected,
  onClick,
  icon,
  title,
  description,
}: {
  selected: boolean
  onClick: () => void
  icon: ReactNode
  title: string
  description?: string
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'flex h-full min-h-[5.5rem] w-full flex-col items-start gap-2 rounded-2xl border p-4 text-left transition',
        selected
          ? 'border-[var(--color-accent)] bg-[var(--color-accent-soft)] ring-1 ring-[var(--color-accent)]/40'
          : 'border-[var(--color-border-strong)] bg-[var(--color-surface-2)] hover:border-[var(--color-muted)]',
      )}
    >
      <span
        className={cn(
          'flex size-8 items-center justify-center rounded-lg',
          selected
            ? 'bg-[var(--color-accent)] text-white'
            : 'bg-[var(--color-surface-elevated)] text-[var(--color-muted)]',
        )}
      >
        {icon}
      </span>
      <span className="text-sm font-medium text-[var(--color-fg)]">{title}</span>
      {description ? (
        <span className="text-xs leading-snug text-[var(--color-muted)]">{description}</span>
      ) : null}
    </button>
  )
}

/** Dashboard-style metric tile. */
export function MetricCard({
  label,
  hint,
  value,
  icon,
  accent,
}: {
  label: string
  hint?: string
  value: string
  icon?: ReactNode
  accent?: 'success' | 'danger' | 'accent'
}) {
  const tone = accent === 'success' ? 'success' : accent === 'danger' ? 'danger' : 'accent'
  return (
    <div
      className={cn(
        'rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] p-5',
        surfaceShadow,
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
            {label}
          </div>
          {hint ? <div className="mt-0.5 text-xs text-[var(--color-muted)]">{hint}</div> : null}
        </div>
        {icon ? (
          <IconBadge tone={tone} size="sm">
            {icon}
          </IconBadge>
        ) : null}
      </div>
      <div
        className={cn(
          'mt-5 text-2xl font-semibold tracking-tight tabular-nums',
          accent === 'danger' ? 'text-[var(--color-danger)]' : 'text-[var(--color-fg)]',
        )}
      >
        {value}
      </div>
    </div>
  )
}

/** @deprecated Prefer MetricCard — kept for any residual imports. */
export function StatCard({
  label,
  value,
  icon,
}: {
  label: string
  value: string
  icon?: ReactNode
}) {
  return <MetricCard label={label} value={value} icon={icon} />
}

export function Segmented<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T
  onChange: (v: T) => void
  options: Array<{ id: T; label: string; icon?: ReactNode }>
}) {
  return (
    <div className="inline-flex h-10 items-stretch rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] p-0.5">
      {options.map((opt) => {
        const active = value === opt.id
        return (
          <button
            key={opt.id}
            type="button"
            onClick={() => onChange(opt.id)}
            className={cn(
              'inline-flex items-center gap-1.5 rounded-[calc(var(--radius-control)-2px)] px-3 text-sm font-medium transition',
              active
                ? 'bg-[var(--color-surface)] text-[var(--color-fg)] shadow-sm'
                : 'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
            )}
          >
            {opt.icon}
            {opt.label}
          </button>
        )
      })}
    </div>
  )
}

/** Horizontal ratio bar used in flow summaries. */
export function FlowBar({
  label,
  value,
  ratio,
  tone,
}: {
  label: string
  value: string
  ratio: number
  tone: 'success' | 'danger' | 'accent'
}) {
  const pct = Math.round(Math.min(1, Math.max(0, ratio)) * 100)
  const bar =
    tone === 'success'
      ? 'bg-[var(--color-success)]'
      : tone === 'danger'
        ? 'bg-[var(--color-danger)]'
        : 'bg-[var(--color-accent)]'
  return (
    <div>
      <div className="mb-1.5 flex items-baseline justify-between gap-3 text-sm">
        <span className="text-[var(--color-muted)]">{label}</span>
        <span className="font-semibold tabular-nums text-[var(--color-fg)]">{value}</span>
      </div>
      <div className="h-2 overflow-hidden rounded bg-[var(--color-surface-elevated)]">
        <div
          className={cn('h-full rounded transition-all duration-500', bar)}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  )
}

/** List row for activity / entity rows with hover. */
export function ListRow({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <li
      className={cn(
        'flex items-center gap-4 px-5 py-3.5 transition hover:bg-[var(--color-surface-2)]/50',
        className,
      )}
    >
      {children}
    </li>
  )
}
