import { ChevronDown, CircleAlert, Loader2 } from 'lucide-react'
import { useState } from 'react'
import type {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
} from 'react'

import { cn } from '../lib/cn'

/**
 * The shared control look. `ui-control` is a styling hook, not decoration:
 * index.css turns a Field around a control into one glass field box and strips
 * the control's own box, so every text-like control must keep the class.
 */
const controlBase =
  'ui-control h-10 w-full rounded-[var(--radius-control)] border border-[var(--color-border)] bg-[var(--color-control)] px-3 text-sm text-[var(--color-fg)] outline-none transition placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent-b)] focus:ring-4 focus:ring-[var(--color-accent-b)]/15 aria-[invalid=true]:border-[var(--color-danger)] disabled:opacity-50'

/** Tinted icon chip used across metric cards and activity rows. */
export function IconBadge({
  children,
  tone = 'accent',
  size = 'md',
  className = '',
}: {
  children: ReactNode
  tone?: 'accent' | 'success' | 'danger' | 'warning' | 'info' | 'muted' | 'money-in' | 'money-out'
  size?: 'xs' | 'sm' | 'md'
  className?: string
}) {
  const tones = {
    accent: 'bg-[var(--color-accent-soft)] text-[var(--color-accent)]',
    success: 'bg-[var(--color-success-soft)] text-[var(--color-success)]',
    danger: 'bg-[var(--color-danger-soft)] text-[var(--color-danger)]',
    warning: 'bg-[var(--color-warning-soft)] text-[var(--color-warning)]',
    info: 'bg-[var(--color-info-soft)] text-[var(--color-info)]',
    muted: 'bg-white/[0.06] text-[var(--color-dim)]',
    // Money identity: never status. Used for income/money-in and
    // expense-or-bill/money-out icons across Dashboard, Transactions,
    // Recurring and Accounts.
    'money-in': 'bg-[var(--color-money-in-soft)] text-[var(--color-money-in-text)]',
    'money-out': 'bg-[var(--color-money-out-soft)] text-[var(--color-money-out-text)]',
  }
  const sizes = {
    xs: 'size-6',
    sm: 'size-8',
    md: 'size-9',
  }

  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-[11px]',
        tones[tone],
        sizes[size],
        className,
      )}
    >
      {children}
    </span>
  )
}

/** The glass panel every block of content sits on. */
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

  return <div className={cn('glass-pane rounded-[22px]', pad, className)}>{children}</div>
}

/**
 * The glass pane for a page's key figure. It carries no colour wash of its
 * own: the aurora shows through, and text keeps its measured contrast.
 * `accent` is still accepted so existing callers compile unchanged.
 */
export function Hero({
  children,
  className = '',
}: {
  children: ReactNode
  className?: string
  accent?: 'accent' | 'success' | 'neutral'
}) {
  return (
    <div className={cn('glass-pane relative overflow-hidden rounded-[24px]', className)}>
      <div className="relative">{children}</div>
    </div>
  )
}

/** Glass panel with a title bar — activity lists, report blocks, etc. */
export function Panel({
  title,
  description,
  whisper,
  icon,
  actions,
  children,
  className = '',
}: {
  title: string
  description?: string
  /** Quieter second line under list meta (export omit reminder). */
  whisper?: string
  icon?: ReactNode
  actions?: ReactNode
  children: ReactNode
  className?: string
}) {
  return (
    <div className={cn('glass-pane overflow-hidden rounded-[22px]', className)}>
      <div className="flex items-center justify-between gap-3 border-b border-[var(--color-border)] px-5 py-4">
        <div className="min-w-0">
          <h3 className="text-[15px] font-semibold text-[var(--color-fg)]">{title}</h3>
          {description ? (
            <p className="text-[13px] text-[var(--color-muted)]">{description}</p>
          ) : null}
          {whisper ? <p className="text-xs text-[var(--color-muted)]">{whisper}</p> : null}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {actions}
          {icon ? <span className="text-[var(--color-dim)]">{icon}</span> : null}
        </div>
      </div>
      {children}
    </div>
  )
}

/**
 * Collapsible glass section: the header row is always visible with a chevron
 * at the end; the body renders only while expanded. Collapsed by default.
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
    <div className="glass-pane overflow-hidden rounded-[22px]">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex w-full items-center gap-3 px-5 py-4 text-left transition hover:bg-white/[0.04]"
      >
        {icon ? (
          <IconBadge tone={tone} size="sm">
            {icon}
          </IconBadge>
        ) : null}
        <div className="min-w-0 flex-1">
          <h3 className="text-[15px] font-semibold text-[var(--color-fg)]">{title}</h3>
          {description ? (
            <p className="text-[13px] text-[var(--color-muted)]">{description}</p>
          ) : null}
        </div>
        <ChevronDown
          className={cn(
            'size-4 shrink-0 text-[var(--color-dim)] transition-transform duration-200',
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
    // Dark ink on the brand light: 11.9:1.
    primary:
      'bg-[linear-gradient(90deg,var(--color-accent),var(--color-accent-b))] font-semibold text-[var(--color-on-accent)] shadow-[0_8px_26px_rgba(46,230,166,0.28),inset_0_1px_0_rgba(255,255,255,0.35)] hover:brightness-110',
    secondary:
      'bg-white/[0.06] text-[var(--color-fg)] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.1),inset_0_1px_0_rgba(255,255,255,0.08)] hover:bg-white/[0.1]',
    // White on the deepened fill: 4.75:1 at its lightest stop. Hover glows
    // instead of brightening, which would drop the label below 4.5:1.
    danger:
      'bg-[linear-gradient(90deg,var(--color-danger-fill-a),var(--color-danger-fill-b))] font-semibold text-white shadow-[0_8px_26px_rgba(215,48,76,0.3),inset_0_1px_0_rgba(255,255,255,0.25)] hover:shadow-[0_10px_34px_rgba(215,48,76,0.5),inset_0_1px_0_rgba(255,255,255,0.25)]',
    ghost: 'text-[var(--color-muted)] hover:bg-white/[0.06] hover:text-[var(--color-fg)]',
  }
  const sizes = {
    sm: 'h-8 gap-1.5 px-3 text-xs',
    md: 'h-10 gap-2 px-4 text-sm',
    icon: 'h-10 w-10 shrink-0 justify-center p-0',
  }

  return (
    <button
      type="button"
      className={cn(
        'inline-flex items-center justify-center rounded-[var(--radius-control)] font-medium transition disabled:cursor-not-allowed disabled:opacity-50 disabled:shadow-none disabled:brightness-100',
        styles[variant],
        sizes[size],
        className,
      )}
      disabled={disabled || busy}
      data-variant={variant}
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
        className="pointer-events-none absolute top-1/2 right-2.5 size-4 -translate-y-1/2 text-[var(--color-dim)]"
        strokeWidth={1.75}
        aria-hidden
      />
    </div>
  )
}

export function Label({ children }: { children: ReactNode }) {
  return (
    <span className="mb-1 block font-mono text-[9.5px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
      {children}
    </span>
  )
}

/** A labelled control. Around a `ui-control` it renders as one glass field box. */
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
    <label className={cn('field-box block min-w-0', className)}>
      <Label>{label}</Label>
      {children}
    </label>
  )
}

/**
 * Page chrome: a mono eyebrow or breadcrumb, a large title, a description and
 * optional actions or meta. It sits directly on the aurora, under the veil.
 */
export function PageHeader({
  eyebrow,
  breadcrumb,
  title,
  description,
  actions,
  meta,
}: {
  eyebrow?: string
  /** Sub-view trail (Transactions / Recurring). Last crumb is the current page. */
  breadcrumb?: Array<{ label: string; onClick?: () => void }>
  title: string
  description?: string
  actions?: ReactNode
  meta?: ReactNode
}) {
  const hasLead = Boolean(eyebrow || (breadcrumb && breadcrumb.length > 0))

  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div className="min-w-0">
        {breadcrumb && breadcrumb.length > 0 ? (
          <nav
            className="flex flex-wrap items-center gap-1.5 font-mono text-[10.5px] font-medium tracking-[0.16em] text-[var(--color-muted)] uppercase"
            aria-label={breadcrumb.map((c) => c.label).join(' / ')}
          >
            {breadcrumb.map((crumb, i) => (
              <span key={`${crumb.label}-${i}`} className="inline-flex items-center gap-1.5">
                {i > 0 ? <span aria-hidden>/</span> : null}
                {crumb.onClick ? (
                  <button
                    type="button"
                    onClick={crumb.onClick}
                    className="hover:text-[var(--color-fg)]"
                  >
                    {crumb.label}
                  </button>
                ) : (
                  <span className="text-[var(--color-fg-secondary)]">{crumb.label}</span>
                )}
              </span>
            ))}
          </nav>
        ) : eyebrow ? (
          <p className="font-mono text-[10.5px] font-medium tracking-[0.16em] text-[var(--color-muted)] uppercase">
            {eyebrow}
          </p>
        ) : null}
        <h2
          className={cn(
            'leading-tight font-semibold tracking-tight text-[var(--color-fg)]',
            hasLead ? 'mt-1.5 text-[1.875rem]' : 'text-[1.625rem]',
          )}
        >
          {title}
        </h2>
        {description ? (
          <p className="mt-1 text-sm text-[var(--color-fg-secondary)]">{description}</p>
        ) : null}
      </div>
      <div className="flex shrink-0 flex-wrap items-center gap-2">
        {meta ? <div className="text-xs text-[var(--color-muted)]">{meta}</div> : null}
        {actions}
      </div>
    </div>
  )
}

export function ErrorBanner({
  message,
  title,
  className = 'mb-4',
  id,
}: {
  message: string | null
  title?: string
  className?: string
  id?: string
}) {
  if (!message && !title) return null

  return (
    <div
      id={id}
      role="alert"
      aria-live="assertive"
      className={cn(
        'flex items-start gap-2 rounded-[14px] bg-[var(--color-danger-soft)] px-4 py-3 text-sm text-[var(--color-danger-text)] shadow-[inset_0_0_0_1px_rgba(255,130,149,0.4)]',
        className,
      )}
    >
      {title ? (
        <CircleAlert className="mt-0.5 size-4 shrink-0 text-[var(--color-danger)]" aria-hidden />
      ) : null}
      <div className="min-w-0 flex-1">
        {title ? <p className="font-semibold">{title}</p> : null}
        {message ? <p className={title ? 'mt-0.5' : undefined}>{message}</p> : null}
      </div>
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
    <div className="glass-pane flex flex-col items-center rounded-[22px] px-6 py-16 text-center">
      {icon ? (
        <div className="oik-empty-icon mb-4 flex size-12 items-center justify-center rounded-[15px] bg-[var(--color-info-soft)] text-[var(--color-info)] shadow-[inset_0_0_0_1px_rgba(55,213,255,0.25),0_0_26px_rgba(55,213,255,0.15)]">
          {icon}
        </div>
      ) : null}
      <p className="oik-empty-title text-sm font-semibold text-[var(--color-fg)]">{title}</p>
      <p className="oik-empty-body mt-1.5 max-w-sm text-sm text-[var(--color-muted)]">{body}</p>
      {action ? <div className="mt-6">{action}</div> : null}
    </div>
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
        'flex h-full min-h-[5.5rem] w-full flex-col items-start gap-2 rounded-[16px] p-4 text-left transition',
        selected
          ? 'bg-[var(--color-accent-soft)] shadow-[inset_0_0_0_1px_rgba(46,230,166,0.45),0_0_24px_rgba(46,230,166,0.12)]'
          : 'bg-white/[0.04] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.08)] hover:bg-white/[0.07]',
      )}
    >
      <span
        className={cn(
          'flex size-8 items-center justify-center rounded-[10px]',
          selected
            ? 'bg-[linear-gradient(135deg,var(--color-accent),var(--color-accent-b))] text-[var(--color-on-accent)]'
            : 'bg-white/[0.06] text-[var(--color-dim)]',
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
  // success/danger here always mean money in/out (income/expense tiles), so
  // the icon wears the Ledger tone, never the status colour.
  const tone = accent === 'success' ? 'money-in' : accent === 'danger' ? 'money-out' : 'accent'

  return (
    <div className="glass-pane rounded-[20px] p-5">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="font-mono text-[10.5px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
            {label}
          </div>
          {hint ? <div className="mt-1 text-xs text-[var(--color-muted)]">{hint}</div> : null}
        </div>
        {icon ? (
          <IconBadge tone={tone} size="sm">
            {icon}
          </IconBadge>
        ) : null}
      </div>
      <div
        title={value}
        className={cn(
          'mt-5 truncate text-[26px] font-semibold tracking-tight tabular-nums',
          // "danger" marks money going out (the Expenses tile): it is set in the
          // Ledger out tint, never in the error colour.
          accent === 'danger' ? 'text-[var(--color-money-out-text)]' : 'text-[var(--color-fg)]',
        )}
      >
        {value}
      </div>
    </div>
  )
}

export function Segmented<T extends string>({
  value,
  onChange,
  options,
  className = '',
}: {
  value: T
  onChange: (v: T) => void
  options: Array<{ id: T; label: string; icon?: ReactNode }>
  className?: string
}) {
  return (
    <div
      className={cn(
        'inline-flex h-10 items-stretch gap-0.5 rounded-full bg-white/[0.04] p-1 shadow-[inset_0_0_0_1px_rgba(255,255,255,0.08)]',
        className,
      )}
    >
      {options.map((opt) => {
        const active = value === opt.id

        return (
          <button
            key={opt.id}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(opt.id)}
            className={cn(
              'inline-flex items-center gap-1.5 rounded-full px-3.5 text-[13px] font-medium transition',
              active
                ? 'bg-white/10 text-[var(--color-fg)] shadow-[inset_0_1px_0_rgba(255,255,255,0.12)]'
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

  // Every caller uses success and danger for money in and out, so the bars
  // wear the Ledger gradients rather than the status colours.
  const bar =
    tone === 'success'
      ? 'bg-[linear-gradient(90deg,#27bf93,var(--color-money-in))]'
      : tone === 'danger'
        ? 'bg-[linear-gradient(90deg,#f07a45,var(--color-money-out))]'
        : 'bg-[linear-gradient(90deg,var(--color-accent),var(--color-accent-b))]'

  return (
    <div>
      <div className="mb-1.5 flex items-baseline justify-between gap-3 text-sm">
        <span className="shrink-0 text-[var(--color-muted)]">{label}</span>
        <span
          title={value}
          className="min-w-0 truncate font-semibold tabular-nums text-[var(--color-fg)]"
        >
          {value}
        </span>
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-white/[0.06]">
        <div
          className={cn('h-full rounded-full transition-all duration-500', bar)}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  )
}

/** List row with a hover surface that also shows for keyboard focus inside it. */
export function ListRow({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <li
      className={cn(
        'flex items-center gap-4 px-5 py-3.5 transition hover:bg-white/[0.04] focus-within:bg-white/[0.04]',
        className,
      )}
    >
      {children}
    </li>
  )
}
