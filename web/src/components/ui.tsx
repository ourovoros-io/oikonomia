import { ChevronDown, CircleAlert, Loader2, X } from 'lucide-react'
import { Children, isValidElement, useEffect, useRef, useState } from 'react'
import type {
  ButtonHTMLAttributes,
  ComponentProps,
  ReactNode,
  SelectHTMLAttributes,
} from 'react'

import { cn } from '../lib/cn'
import { useI18n } from '../lib/I18nProvider'

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
        'inline-flex shrink-0 items-center justify-center rounded-[12px]',
        tones[tone],
        sizes[size],
        className,
      )}
    >
      {children}
    </span>
  )
}

/**
 * A labelled money figure on a tinted plate: IN and OUT in the Ledger tints,
 * neutral for balances. The label tier is measured to hold 4.5:1 on every
 * plate (tokens.test.ts).
 */
export function MoneyPill({
  tone,
  label,
  value,
  size = 'md',
}: {
  tone: 'in' | 'out' | 'neutral'
  label: string
  value: string
  size?: 'sm' | 'md'
}) {
  const tones = {
    in: 'bg-[var(--color-money-in-soft)] text-[var(--color-money-in-text)] shadow-[inset_0_0_0_1px_rgba(27,163,154,0.4)]',
    out: 'bg-[var(--color-money-out-soft)] text-[var(--color-money-out-text)] shadow-[inset_0_0_0_1px_rgba(232,96,63,0.4)]',
    neutral:
      'bg-[var(--color-plate-neutral)] text-[var(--color-fg)] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.12)]',
  }

  return (
    <span
      data-money-pill={tone}
      className={cn(
        'inline-flex items-center gap-2 rounded-full font-semibold whitespace-nowrap tabular-nums',
        size === 'sm' ? 'h-7 px-2.5 text-xs' : 'h-8 px-3 text-sm',
        tones[tone],
      )}
    >
      <span className="font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-fg-secondary)] uppercase">
        {label}
      </span>{' '}
      {value}
    </span>
  )
}

/** A signed amount on a tinted plate, for list rows. */
export function AmountPill({
  tone,
  children,
}: {
  tone: 'in' | 'out' | 'neutral'
  children: ReactNode
}) {
  const tones = {
    in: 'bg-[var(--color-money-in-soft)] text-[var(--color-money-in-text)]',
    out: 'bg-[var(--color-money-out-soft)] text-[var(--color-money-out-text)]',
    neutral: 'bg-[var(--color-plate-neutral)] text-[var(--color-fg)]',
  }

  return (
    <span
      data-amount={tone}
      className={cn(
        'inline-flex h-8 shrink-0 items-center rounded-full px-3 text-sm font-semibold whitespace-nowrap tabular-nums',
        tones[tone],
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
    lg: 'p-6',
  }[padding]

  return <div className={cn('glass-pane rounded-[20px]', pad, className)}>{children}</div>
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
    <div className={cn('glass-pane overflow-hidden rounded-[20px]', className)}>
      <div className="flex items-center justify-between gap-3 border-b border-[var(--color-border)] px-5 py-4">
        <div className="min-w-0">
          <h3 className="text-base leading-6 font-semibold text-[var(--color-fg)]">{title}</h3>
          {description ? (
            <p className="text-[13px] leading-5 text-[var(--color-muted)]">{description}</p>
          ) : null}
          {whisper ? <p className="text-xs leading-4 text-[var(--color-muted)]">{whisper}</p> : null}
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
  summary,
  children,
}: {
  title: string
  description?: string
  icon?: ReactNode
  /** Chrome tones only: success means a confirmation, never a section. */
  tone?: 'accent' | 'danger' | 'warning' | 'info' | 'muted'
  defaultOpen?: boolean
  /** Body without padding, for lists that manage their own edges. */
  flush?: boolean
  /** The current value of a setting, shown in the header so it reads while collapsed. */
  summary?: string
  children: ReactNode
}) {
  const [open, setOpen] = useState(defaultOpen)

  return (
    <div className="glass-pane overflow-hidden rounded-[20px]">
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
          <h3 className="text-base leading-6 font-semibold text-[var(--color-fg)]">{title}</h3>
          {description ? (
            <p className="text-[13px] leading-5 text-[var(--color-muted)]">{description}</p>
          ) : null}
        </div>
        {summary ? (
          <span className="shrink-0 text-[13px] font-medium whitespace-nowrap text-[var(--color-fg-secondary)]">
            {summary}
          </span>
        ) : null}
        <ChevronDown
          className={cn(
            'size-4 shrink-0 text-[var(--color-dim)] transition-transform duration-200',
            open && 'rotate-180',
          )}
        />
      </button>

      {open ? (
        <div className={cn('border-t border-[var(--color-border)]', !flush && 'p-5')}>
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
  /** `icon` is a 40px square beside default controls; `iconSm` a 32px one for rows and dialog chrome. */
  size?: 'sm' | 'md' | 'lg' | 'icon' | 'iconSm'
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
    // Auth screens only: the one primary action on the page.
    lg: 'h-12 gap-2 px-5 text-sm',
    icon: 'h-10 w-10 shrink-0 justify-center p-0',
    iconSm: 'h-8 w-8 shrink-0 justify-center p-0',
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

// ComponentProps, not InputHTMLAttributes: in React 19 it is what carries `ref`.
export function Input({ className = '', ...props }: ComponentProps<'input'>) {
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
        className="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-[var(--color-dim)]"
        strokeWidth={1.75}
        aria-hidden
      />
    </div>
  )
}

export function Label({ children }: { children: ReactNode }) {
  return (
    <span className="mb-1 block font-mono text-[11px] leading-[14px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
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
 * An error or notice that stays until the problem is fixed. With `onDismiss`
 * it can also be closed; without it, the owner clears it. It scrolls itself
 * into view when it appears, so a failure above the fold is never silent.
 */
export function ErrorBanner({
  message,
  title,
  className = 'mb-4',
  id,
  onDismiss,
}: {
  message: string | null
  title?: string
  className?: string
  id?: string
  onDismiss?: () => void
}) {
  const { t } = useI18n()
  const ref = useRef<HTMLDivElement>(null)
  const visible = Boolean(message || title)

  useEffect(() => {
    // Undefined in jsdom.
    if (visible) ref.current?.scrollIntoView?.({ block: 'nearest' })
  }, [visible, message])

  if (!visible) return null

  return (
    <div
      ref={ref}
      id={id}
      role="alert"
      aria-live="assertive"
      className={cn(
        'flex items-start gap-2 rounded-[12px] bg-[var(--color-danger-soft)] px-4 py-3 text-sm text-[var(--color-danger-text)] shadow-[inset_0_0_0_1px_rgba(255,130,149,0.4)]',
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
      {onDismiss ? (
        <button
          type="button"
          onClick={onDismiss}
          aria-label={t('form.dismiss')}
          className="-m-1 shrink-0 rounded-md p-1 hover:bg-white/10"
        >
          <X className="size-4" aria-hidden />
        </button>
      ) : null}
    </div>
  )
}

/**
 * A confirmation that something worked. It goes away by itself after
 * `autoDismissMs`, and can be closed sooner: unlike an error it asks nothing
 * of the reader, so it must not stay on the page for good.
 */
export function Notice({
  message,
  onDismiss,
  autoDismissMs = 8000,
  className = '',
}: {
  message: string | null
  onDismiss: () => void
  autoDismissMs?: number
  className?: string
}) {
  const { t } = useI18n()
  const ref = useRef<HTMLDivElement>(null)
  // Held in a ref so a new closure on each render does not restart the timer.
  const dismissRef = useRef(onDismiss)
  useEffect(() => {
    dismissRef.current = onDismiss
  }, [onDismiss])

  useEffect(() => {
    if (!message) return
    ref.current?.scrollIntoView?.({ block: 'nearest' })
    const timer = window.setTimeout(() => dismissRef.current(), autoDismissMs)

    return () => window.clearTimeout(timer)
  }, [message, autoDismissMs])

  if (!message) return null

  return (
    <div
      ref={ref}
      role="status"
      aria-live="polite"
      className={cn(
        'flex items-start gap-2 rounded-xl border border-[var(--color-accent)]/25 bg-[var(--color-accent-soft)] px-4 py-3 text-sm text-[var(--color-fg-secondary)]',
        className,
      )}
    >
      <p className="min-w-0 flex-1">{message}</p>
      <button
        type="button"
        onClick={onDismiss}
        aria-label={t('form.dismiss')}
        className="-m-1 shrink-0 rounded-md p-1 hover:bg-white/10"
      >
        <X className="size-4" aria-hidden />
      </button>
    </div>
  )
}

/**
 * Floats page-level banners over the top of the main pane instead of pushing
 * the page down when one appears. The stack is a zero-height sticky strip, so
 * it takes no room in the layout and stays in view while the pane scrolls.
 * Each child gets a solid surface behind it (the banners are translucent
 * tints); a child that renders nothing leaves an empty, hidden wrapper.
 */
export function ToastStack({ children }: { children: ReactNode }) {
  return (
    <div className="oik-toast-stack">
      {Children.toArray(children).map((child) => (
        <div key={isValidElement(child) ? child.key : undefined} className="oik-toast">
          {child}
        </div>
      ))}
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
    <div className="glass-pane flex flex-col items-center rounded-[20px] px-6 py-16 text-center">
      {icon ? (
        <div className="oik-empty-icon mb-4 flex size-12 items-center justify-center rounded-[16px] bg-[var(--color-info-soft)] text-[var(--color-info)] shadow-[inset_0_0_0_1px_rgba(55,213,255,0.25),0_0_26px_rgba(55,213,255,0.15)]">
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
          'flex size-8 items-center justify-center rounded-[12px]',
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
          <div className="font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
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
          'mt-5 truncate text-2xl font-semibold tracking-tight tabular-nums',
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
  /**
   * `tone` colours an option while it is chosen: an entry type that is money
   * in or out wears that Ledger gradient; untoned options take the neutral plate.
   */
  options: Array<{ id: T; label: string; icon?: ReactNode; tone?: 'money-in' | 'money-out' }>
  className?: string
}) {
  const toned = {
    'money-in':
      'bg-[linear-gradient(90deg,var(--color-money-in-a),var(--color-money-in-b)_55%,var(--color-money-in-c))] font-semibold text-[var(--color-on-money-in)] shadow-[0_6px_22px_rgba(27,163,154,0.35)]',
    'money-out':
      'bg-[linear-gradient(90deg,var(--color-money-out-a),var(--color-money-out-b)_55%,var(--color-money-out-c))] font-semibold text-[var(--color-on-money-out)] shadow-[0_6px_22px_rgba(232,96,63,0.35)]',
  }

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
            data-tone={active ? (opt.tone ?? 'neutral') : undefined}
            onClick={() => onChange(opt.id)}
            className={cn(
              'inline-flex items-center gap-1.5 rounded-full px-3.5 text-[13px] font-medium transition focus-visible:outline-offset-[-2px]',
              active
                ? opt.tone
                  ? toned[opt.tone]
                  : 'bg-white/10 text-[var(--color-fg)] shadow-[inset_0_1px_0_rgba(255,255,255,0.12)]'
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

/** List row with a hover surface that also shows for keyboard focus inside it. */
export function ListRow({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <li
      className={cn(
        'flex items-center gap-4 px-5 py-3 transition hover:bg-white/[0.04] focus-within:bg-white/[0.04]',
        className,
      )}
    >
      {children}
    </li>
  )
}
