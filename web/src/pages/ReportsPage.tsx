import { useEffect, useState } from 'react'
import {
  BarChart3,
  FileSpreadsheet,
  RefreshCw,
  Scale,
  TrendingDown,
  TrendingUp,
} from 'lucide-react'
import {
  api,
  formatDate,
  formatMoney,
  todayISO,
  yearStartISO,
  type BalanceSheet,
  type Entity,
  type PnL,
  type ReportLine,
  type TrialBalance,
} from '../lib/api'
import {
  Button,
  Card,
  EmptyState,
  ErrorBanner,
  Field,
  Hero,
  Input,
  MetricCard,
  PageHeader,
  Panel,
  Segmented,
  cn,
} from '../components/ui'
import type { CommandError } from '../lib/tauri'

type Props = { entity: Entity | null }
type Tab = 'trial' | 'pnl' | 'bs'

export function ReportsPage({ entity }: Props) {
  const [tab, setTab] = useState<Tab>('pnl')
  const [asOf, setAsOf] = useState(todayISO())
  const [from, setFrom] = useState(yearStartISO())
  const [to, setTo] = useState(todayISO())
  const [error, setError] = useState<string | null>(null)
  const [tb, setTb] = useState<TrialBalance | null>(null)
  const [pnl, setPnl] = useState<PnL | null>(null)
  const [bs, setBs] = useState<BalanceSheet | null>(null)

  async function run() {
    if (!entity) return
    setError(null)
    try {
      if (tab === 'trial') setTb(await api.reportTrialBalance(entity.id, asOf))
      if (tab === 'pnl') setPnl(await api.reportPnl(entity.id, from, to))
      if (tab === 'bs') setBs(await api.reportBalanceSheet(entity.id, asOf))
    } catch (err) {
      setError((err as CommandError).message)
    }
  }

  useEffect(() => {
    if (entity) void run()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity?.id, tab])

  if (!entity) {
    return (
      <EmptyState
        icon={<BarChart3 className="size-5" />}
        title="No book selected"
        body="Create or select a book first."
      />
    )
  }

  const ccy = entity.base_currency

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Statements"
        title="Reports"
        description="Industry-standard P&L, balance sheet, and trial balance"
        meta={entity.name}
        actions={
          <Segmented<Tab>
            value={tab}
            onChange={setTab}
            options={[
              { id: 'pnl', label: 'P&L', icon: <BarChart3 className="size-3.5" /> },
              { id: 'bs', label: 'Balance sheet', icon: <Scale className="size-3.5" /> },
              {
                id: 'trial',
                label: 'Trial balance',
                icon: <FileSpreadsheet className="size-3.5" />,
              },
            ]}
          />
        }
      />

      <Card>
        <div className="flex flex-wrap items-end gap-3">
          {tab === 'pnl' ? (
            <>
              <Field label="From" className="w-[11rem]">
                <Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
              </Field>
              <Field label="To" className="w-[11rem]">
                <Input type="date" value={to} onChange={(e) => setTo(e.target.value)} />
              </Field>
            </>
          ) : (
            <Field label="As of" className="w-[11rem]">
              <Input type="date" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
            </Field>
          )}
          <Button variant="secondary" onClick={() => void run()}>
            <RefreshCw className="size-4" />
            Refresh
          </Button>
        </div>
      </Card>

      <ErrorBanner message={error} />

      {tab === 'trial' && tb ? <TrialView tb={tb} ccy={ccy} /> : null}
      {tab === 'pnl' && pnl ? <PnlView pnl={pnl} ccy={ccy} /> : null}
      {tab === 'bs' && bs ? <BsView bs={bs} ccy={ccy} /> : null}
    </div>
  )
}

function LinesList({ lines, ccy }: { lines: ReportLine[]; ccy: string }) {
  if (lines.length === 0) {
    return (
      <div className="px-5 py-8 text-center text-sm text-[var(--color-muted)]">No lines</div>
    )
  }
  return (
    <ul className="divide-y divide-[var(--color-border)]">
      {lines.map((l) => (
        <li
          key={`${l.code}-${l.name}`}
          className="flex items-center gap-4 px-5 py-3 transition hover:bg-[var(--color-surface-2)]/50"
        >
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-medium text-[var(--color-fg)]">{l.name}</div>
            <div className="text-xs tabular-nums text-[var(--color-muted)]">{l.code}</div>
          </div>
          <div className="shrink-0 text-sm font-semibold tabular-nums text-[var(--color-fg)]">
            {formatMoney(l.balance_minor, ccy)}
          </div>
        </li>
      ))}
    </ul>
  )
}

function TrialView({ tb, ccy }: { tb: TrialBalance; ccy: string }) {
  return (
    <div className="space-y-4">
      <p className="text-sm text-[var(--color-muted)]">As of {formatDate(tb.as_of)}</p>
      <div className="grid gap-3 sm:grid-cols-2">
        <MetricCard
          label="Total debits"
          value={formatMoney(tb.total_debits, ccy)}
          icon={<TrendingUp className="size-4" />}
        />
        <MetricCard
          label="Total credits"
          value={formatMoney(tb.total_credits, ccy)}
          icon={<TrendingDown className="size-4" />}
        />
      </div>
      <Panel title="Trial balance" description="Debits and credits by account">
        <ul className="divide-y divide-[var(--color-border)]">
          {tb.lines.map((l) => (
            <li
              key={l.code + l.name}
              className="flex items-center gap-4 px-5 py-3 transition hover:bg-[var(--color-surface-2)]/50"
            >
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-medium text-[var(--color-fg)]">
                  {l.name}
                </div>
                <div className="text-xs tabular-nums text-[var(--color-muted)]">{l.code}</div>
              </div>
              <div className="w-28 shrink-0 text-right text-sm tabular-nums text-[var(--color-fg)]">
                {l.debit_minor ? formatMoney(l.debit_minor, ccy) : '—'}
              </div>
              <div className="w-28 shrink-0 text-right text-sm tabular-nums text-[var(--color-fg)]">
                {l.credit_minor ? formatMoney(l.credit_minor, ccy) : '—'}
              </div>
            </li>
          ))}
          <li className="flex items-center gap-4 bg-[var(--color-surface-2)]/40 px-5 py-3.5">
            <div className="min-w-0 flex-1 text-sm font-semibold text-[var(--color-fg)]">
              Total
            </div>
            <div className="w-28 shrink-0 text-right text-sm font-semibold tabular-nums text-[var(--color-fg)]">
              {formatMoney(tb.total_debits, ccy)}
            </div>
            <div className="w-28 shrink-0 text-right text-sm font-semibold tabular-nums text-[var(--color-fg)]">
              {formatMoney(tb.total_credits, ccy)}
            </div>
          </li>
        </ul>
      </Panel>
    </div>
  )
}

function PnlView({ pnl, ccy }: { pnl: PnL; ccy: string }) {
  const net = pnl.net_income
  return (
    <div className="space-y-6">
      <Hero accent={net < 0 ? 'neutral' : 'success'}>
        <div className="p-6 sm:p-8">
          <div className="flex items-center gap-2 text-sm text-[var(--color-muted)]">
            <BarChart3 className="size-4 text-[var(--color-accent)]" />
            Net income
          </div>
          <div
            className={cn(
              'mt-3 text-4xl font-semibold tracking-tight tabular-nums sm:text-5xl',
              net < 0 ? 'text-[var(--color-danger)]' : 'text-[var(--color-fg)]',
            )}
          >
            {formatMoney(net, ccy)}
          </div>
          <p className="mt-3 text-sm text-[var(--color-muted)]">
            {formatDate(pnl.from)} → {formatDate(pnl.to)}
          </p>
        </div>
      </Hero>

      <div className="grid gap-3 sm:grid-cols-2">
        <MetricCard
          label="Total income"
          hint="Period"
          value={formatMoney(pnl.total_income, ccy)}
          icon={<TrendingUp className="size-4" />}
          accent="success"
        />
        <MetricCard
          label="Total expenses"
          hint="Period"
          value={formatMoney(pnl.total_expenses, ccy)}
          icon={<TrendingDown className="size-4" />}
          accent="danger"
        />
      </div>

      <Panel title="Income" description="Revenue accounts">
        <LinesList lines={pnl.income} ccy={ccy} />
      </Panel>
      <Panel title="Expenses" description="Cost accounts">
        <LinesList lines={pnl.expenses} ccy={ccy} />
      </Panel>
    </div>
  )
}

function BsView({ bs, ccy }: { bs: BalanceSheet; ccy: string }) {
  return (
    <div className="space-y-6">
      <p className="text-sm text-[var(--color-muted)]">As of {formatDate(bs.as_of)}</p>

      <div className="grid gap-3 sm:grid-cols-2">
        <MetricCard
          label="Total assets"
          value={formatMoney(bs.total_assets, ccy)}
          icon={<Scale className="size-4" />}
        />
        <MetricCard
          label="Liabilities + equity"
          value={formatMoney(bs.total_liabilities_equity, ccy)}
          icon={<FileSpreadsheet className="size-4" />}
        />
      </div>

      {([bs.assets, bs.liabilities, bs.equity] as const).map((section) => (
        <Panel
          key={section.title}
          title={section.title}
          description={`Total ${formatMoney(section.total, ccy)}`}
        >
          <LinesList lines={section.lines} ccy={ccy} />
        </Panel>
      ))}
    </div>
  )
}
