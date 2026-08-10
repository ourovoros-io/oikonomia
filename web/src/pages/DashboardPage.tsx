import { useEffect, useMemo, useState } from 'react'
import {
  ArrowDownLeft,
  ArrowUpRight,
  Landmark,
  Receipt,
  Scale,
  Sparkles,
  TrendingUp,
  Wallet,
} from 'lucide-react'
import {
  api,
  formatDate,
  formatMoney,
  localeForCurrency,
  monthEndISO,
  monthStartISO,
  todayISO,
  type Account,
  type DashboardSummary,
  type Entity,
  type PostedEntryView,
} from '../lib/api'
import {
  EmptyState,
  ErrorBanner,
  FlowBar,
  Hero,
  IconBadge,
  ListRow,
  MetricCard,
  Panel,
  cn,
} from '../components/ui'
import type { CommandError } from '../lib/tauri'

type Props = { entity: Entity | null }

type ActivityRow = {
  id: string
  date: string
  description: string
  signedMinor: number
  kind: 'income' | 'expense' | 'transfer' | 'other'
}

function inferActivity(view: PostedEntryView, accounts: Account[]): ActivityRow {
  const map = new Map(accounts.map((a) => [a.id, a]))
  const types = view.lines.map((l) => map.get(l.account_id)?.account_type)
  const amount = view.lines.reduce((s, l) => s + l.debit.amount_minor, 0)
  let kind: ActivityRow['kind'] = 'other'
  let signed = amount
  if (types.includes('expense')) {
    kind = 'expense'
    signed = -amount
  } else if (types.includes('income')) {
    kind = 'income'
    signed = amount
  } else if (types.every((t) => t === 'asset' || t === 'liability')) {
    kind = 'transfer'
  }
  return {
    id: view.entry.id,
    date: formatDate(view.entry.entry_date),
    description: view.entry.description,
    signedMinor: signed,
    kind,
  }
}

export function DashboardPage({ entity }: Props) {
  const [data, setData] = useState<DashboardSummary | null>(null)
  const [entries, setEntries] = useState<PostedEntryView[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  // Full calendar month, so bills posted with a future due date (common for
  // scanned utility bills) count toward this month's figures immediately.
  // Assets stay "as of today".
  const from = monthStartISO()
  const to = monthEndISO()
  const assetsAsOf = todayISO()

  useEffect(() => {
    if (!entity) {
      setData(null)
      setEntries([])
      setAccounts([])
      return
    }
    let cancelled = false
    setLoading(true)
    void (async () => {
      try {
        const [summary, list, accts] = await Promise.all([
          api.dashboardSummary(entity.id, from, to, assetsAsOf),
          api.entryList(entity.id, from, to),
          api.accountList(entity.id),
        ])
        if (!cancelled) {
          setData(summary)
          setEntries(list)
          setAccounts(accts)
          setError(null)
        }
      } catch (err) {
        if (!cancelled) setError((err as CommandError).message)
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [entity, from, to])

  const activity = useMemo(() => {
    return entries
      .filter((e) => !e.is_voided)
      .slice(0, 8)
      .map((e) => inferActivity(e, accounts))
  }, [entries, accounts])

  if (!entity) {
    return (
      <EmptyState
        icon={<Landmark className="size-5" />}
        title="Create a book to begin"
        body="Add a personal or company entity under Settings. Each entity has its own chart of accounts and reports."
      />
    )
  }

  const ccy = entity.base_currency
  const loc = localeForCurrency(ccy)
  const money = (n: number, signed = false) => formatMoney(n, ccy, loc, { signed })

  const income = data?.income ?? 0
  const expenses = data?.expenses ?? 0
  const net = data?.net_income ?? 0
  const assets = data?.cash_like_assets ?? 0
  const maxFlow = Math.max(income, expenses, 1)

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
            Overview
          </p>
          <h2 className="mt-1 text-[1.75rem] leading-tight font-semibold tracking-tight text-[var(--color-fg)]">
            {entity.name}
          </h2>
          <p className="mt-1 text-sm text-[var(--color-muted)]">
            {from} → {to} · {ccy}
          </p>
        </div>
        <div className="text-xs text-[var(--color-muted)]">Encrypted vault · local only</div>
      </div>

      <ErrorBanner message={error} />

      <Hero>
        <div className="grid gap-8 p-6 sm:grid-cols-[1.2fr_1fr] sm:p-8">
          <div>
            <div className="flex items-center gap-2 text-sm text-[var(--color-muted)]">
              <TrendingUp className="size-4 text-[var(--color-accent)]" />
              Net this month
            </div>
            <div
              className={cn(
                'mt-3 text-4xl font-semibold tracking-tight tabular-nums sm:text-5xl',
                net < 0 ? 'text-[var(--color-danger)]' : 'text-[var(--color-fg)]',
              )}
            >
              {loading && !data ? '—' : money(net, true)}
            </div>
            <p className="mt-3 max-w-md text-sm leading-relaxed text-[var(--color-muted)]">
              Income minus expenses for the current month. Drop bills on Transactions for offline
              OCR, or post entries manually.
            </p>
            <div className="mt-6 flex flex-wrap items-center gap-4 text-xs text-[var(--color-muted)]">
              <span className="inline-flex items-center gap-1.5">
                <Sparkles className="size-3.5 text-[var(--color-accent)]" />
                Offline invoice reader
              </span>
              <span className="inline-flex items-center gap-1.5">
                <Receipt className="size-3.5" />
                {data?.recent_entry_count ?? 0} entries this month
              </span>
            </div>
          </div>

          <div className="flex flex-col justify-center gap-5 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)]/70 p-5 backdrop-blur">
            <FlowBar
              label="Income"
              value={money(income)}
              ratio={income / maxFlow}
              tone="success"
            />
            <FlowBar
              label="Expenses"
              value={money(expenses)}
              ratio={expenses / maxFlow}
              tone="danger"
            />
            <div className="border-t border-[var(--color-border)] pt-4 text-xs text-[var(--color-muted)]">
              Cash-in vs cash-out (period activity). Assets below are balances as of today.
            </div>
          </div>
        </div>
      </Hero>

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <MetricCard
          label="Total assets"
          hint="As of today"
          value={loading && !data ? '—' : money(assets)}
          icon={<Wallet className="size-4" />}
        />
        <MetricCard
          label="Income"
          hint="Month to date"
          value={loading && !data ? '—' : money(income)}
          icon={<ArrowDownLeft className="size-4" />}
          accent="success"
        />
        <MetricCard
          label="Expenses"
          hint="Month to date"
          value={loading && !data ? '—' : money(expenses)}
          icon={<ArrowUpRight className="size-4" />}
          accent="danger"
        />
        <MetricCard
          label="Net result"
          hint="Income − expenses"
          value={loading && !data ? '—' : money(net, true)}
          icon={<Scale className="size-4" />}
          accent={net < 0 ? 'danger' : 'success'}
        />
      </div>

      <Panel
        title="Recent activity"
        description="Posted entries this month"
        icon={<Receipt className="size-4" />}
      >
        {activity.length === 0 ? (
          <div className="px-5 py-12 text-center text-sm text-[var(--color-muted)]">
            {loading ? 'Loading…' : 'No entries yet this month. Post one under Transactions.'}
          </div>
        ) : (
          <ul className="divide-y divide-[var(--color-border)]">
            {activity.map((row) => (
              <ListRow key={row.id}>
                <IconBadge
                  tone={
                    row.kind === 'income'
                      ? 'success'
                      : row.kind === 'expense'
                        ? 'danger'
                        : 'muted'
                  }
                >
                  {row.kind === 'income' ? (
                    <ArrowDownLeft className="size-4" />
                  ) : row.kind === 'expense' ? (
                    <ArrowUpRight className="size-4" />
                  ) : (
                    <Landmark className="size-4" />
                  )}
                </IconBadge>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium text-[var(--color-fg)]">
                    {row.description}
                  </div>
                  <div className="text-xs text-[var(--color-muted)]">
                    {row.date}
                    <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                    <span className="capitalize">{row.kind}</span>
                  </div>
                </div>
                <div
                  className={cn(
                    'shrink-0 text-sm font-semibold tabular-nums',
                    row.signedMinor < 0
                      ? 'text-[var(--color-danger)]'
                      : row.signedMinor > 0 && row.kind === 'income'
                        ? 'text-[var(--color-success)]'
                        : 'text-[var(--color-fg)]',
                  )}
                >
                  {money(row.signedMinor, row.kind === 'income' || row.kind === 'expense')}
                </div>
              </ListRow>
            ))}
          </ul>
        )}
      </Panel>
    </div>
  )
}
