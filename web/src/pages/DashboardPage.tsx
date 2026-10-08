import { useEffect, useMemo, useState } from 'react'
import { ArrowDownLeft, ArrowUpRight, Landmark, Receipt } from 'lucide-react'
import {
  api,
  formatDate,
  bookCurrency,
  formatMoney,
  localeForCurrency,
  monthEndISO,
  monthStartISO,
  quarterEndISO,
  quarterStartISO,
  todayISO,
  yearEndISO,
  yearStartISO,
  type Account,
  type CashFlowSeries,
  type DashboardSummary,
  type Entity,
  type PostedEntryView,
} from '../lib/api'
import { formatPercentFromBps } from '../lib/arc'
import { ArcTile } from '../components/Arc'
import { CashFlowPulse } from '../components/CashFlowPulse'
import { HiddenIncludedNote } from '../components/hiddenUi'
import { TopBar } from '../components/TopBar'
import {
  AmountPill,
  Button,
  EmptyState,
  ErrorBanner,
  Hero,
  IconBadge,
  ListRow,
  MoneyPill,
  Panel,
  Segmented,
} from '../components/ui'
import { cn } from '../lib/cn'
import { commandErrorMessage } from '../lib/commandError'
import { useI18n } from '../lib/I18nProvider'

type Props = { entity: Entity | null; onCreateBook?: () => void }

type Period = 'month' | 'quarter' | 'year'

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

/**
 * The full calendar period containing today, so entries dated ahead (scanned
 * bills carry their due date) count toward it immediately.
 */
function periodBounds(period: Period): { from: string; to: string } {
  if (period === 'month') return { from: monthStartISO(), to: monthEndISO() }
  if (period === 'quarter') return { from: quarterStartISO(), to: quarterEndISO() }
  return { from: yearStartISO(), to: yearEndISO() }
}

export function DashboardPage({ entity, onCreateBook }: Props) {
  const { t, locale } = useI18n()
  const [data, setData] = useState<DashboardSummary | null>(null)
  const [series, setSeries] = useState<CashFlowSeries | null>(null)
  const [entries, setEntries] = useState<PostedEntryView[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [period, setPeriod] = useState<Period>('month')

  const { from, to } = periodBounds(period)
  const assetsAsOf = todayISO()
  const periodWord = t(`dashboard.period.word.${period}`)

  const periodTitle = useMemo(() => {
    const start = new Date(`${from}T12:00:00`)
    if (period === 'month') {
      return new Intl.DateTimeFormat(locale, { month: 'long', year: 'numeric' }).format(start)
    }
    if (period === 'quarter') {
      return t('dashboard.period.quarterTitle', {
        quarter: Math.floor(start.getMonth() / 3) + 1,
        year: start.getFullYear(),
      })
    }
    return String(start.getFullYear())
  }, [from, period, locale, t])

  useEffect(() => {
    if (!entity) {
      setData(null)
      setSeries(null)
      setEntries([])
      setAccounts([])
      return
    }
    let cancelled = false
    setLoading(true)
    void (async () => {
      try {
        const [summary, flow, list, accts] = await Promise.all([
          api.dashboardSummary(entity.id, from, to, assetsAsOf),
          api.cashFlowSeries(entity.id, from, to),
          api.entryList(entity.id, { from, to }),
          api.accountList(entity.id),
        ])
        if (!cancelled) {
          setData(summary)
          setSeries(flow)
          setEntries(list)
          setAccounts(accts)
          setError(null)
        }
      } catch (err) {
        if (!cancelled) setError(commandErrorMessage(err))
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
    // assetsAsOf follows the same clock as from/to; refetching on it alone adds nothing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
        title={t('dash.createTitle')}
        body={t('dash.createBody')}
        action={
          onCreateBook ? (
            <Button onClick={onCreateBook}>{t('empty.createBook')}</Button>
          ) : undefined
        }
      />
    )
  }

  const ccy = bookCurrency(entity)
  const loc = localeForCurrency(ccy.code)
  const money = (n: number, signed = false) => formatMoney(n, ccy, loc, { signed })

  const pending = loading && !data
  const income = data?.income ?? 0
  const expenses = data?.expenses ?? 0
  const net = data?.net_income ?? 0
  const assets = data?.cash_like_assets ?? 0
  const savings = data?.savings_rate_bps ?? null
  const previous = data?.net_vs_previous_bps ?? null
  const top = data?.top_expense ?? null
  const netTone = pending || net === 0 ? 'zero' : net > 0 ? 'in' : 'out'
  const noValue = t('dashboard.arc.noValue')

  return (
    <div className="space-y-4">
      <TopBar
        title={entity.name}
        subtitle={`${periodTitle} · ${ccy.code}`}
        actions={
          <Segmented<Period>
            value={period}
            onChange={setPeriod}
            options={[
              { id: 'month', label: t('dashboard.period.month') },
              { id: 'quarter', label: t('dashboard.period.quarter') },
              { id: 'year', label: t('dashboard.period.year') },
            ]}
          />
        }
      />

      <ErrorBanner message={error} />

      <Hero>
        <div className="flex flex-wrap items-start justify-between gap-6 px-7 pt-6">
          <div className="min-w-0">
            <p className="font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
              {t('dash.netThis', { period: periodWord })}
            </p>
            <p
              data-net={netTone}
              title={money(net, true)}
              className={cn(
                'mt-3 truncate text-[clamp(2.75rem,5.2vw,4rem)] leading-none font-semibold tracking-[-0.02em] tabular-nums',
                netTone === 'in'
                  ? 'net-figure-in'
                  : netTone === 'out'
                    ? 'net-figure-out'
                    : 'text-[var(--color-fg)]',
              )}
            >
              {pending ? '—' : money(net, true)}
            </p>
            {savings !== null && savings > 0 ? (
              <p className="mt-3 text-sm text-[var(--color-fg-secondary)]">
                {t('dashboard.hero.kept', { percent: formatPercentFromBps(savings, locale) })}
              </p>
            ) : null}
            <HiddenIncludedNote count={data?.hidden_entry_count ?? 0} className="mt-3" />
          </div>
          <div className="flex shrink-0 flex-col items-end gap-2">
            <MoneyPill tone="in" label={t('dashboard.pill.in')} value={pending ? '—' : money(income)} />
            <MoneyPill tone="out" label={t('dashboard.pill.out')} value={pending ? '—' : money(expenses)} />
            <MoneyPill tone="neutral" label={t('dashboard.pill.assets')} value={pending ? '—' : money(assets)} />
          </div>
        </div>
        <CashFlowPulse
          series={series}
          formatAmount={(minor) => money(minor)}
          className="mx-7 mt-4 mb-6 h-[132px]"
          label={
            data
              ? t('dashboard.light.label', {
                  income: money(income),
                  expenses: money(expenses),
                  net: money(net, true),
                  range: periodTitle,
                })
              : t('dashboard.light.empty')
          }
        />
      </Hero>

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <ArcTile
          label={t('dashboard.arc.savings.label')}
          hint={t('dashboard.arc.savings.hint')}
          bps={savings}
          tone="in"
          locale={locale}
          noValueLabel={noValue}
        />
        <ArcTile
          label={t('dashboard.arc.spend.label')}
          hint={t('dashboard.arc.spend.hint')}
          bps={data?.spend_ratio_bps ?? null}
          tone="out"
          locale={locale}
          noValueLabel={noValue}
        />
        <ArcTile
          label={t('dashboard.arc.previous.label')}
          hint={t(`dashboard.arc.previous.hint.${period}`)}
          bps={previous}
          signed
          tone={previous !== null && previous < 0 ? 'out' : 'in'}
          locale={locale}
          noValueLabel={noValue}
        />
        <ArcTile
          label={t('dashboard.arc.top.label')}
          hint={top ? top.name : t('dashboard.arc.top.none')}
          bps={top ? top.share_bps : null}
          tone="out"
          locale={locale}
          noValueLabel={noValue}
        />
      </div>

      <Panel
        title={t('dash.recentActivity')}
        description={t('dash.entriesThis', {
          count: data?.recent_entry_count ?? 0,
          period: periodWord,
        })}
        icon={<Receipt className="size-4" />}
      >
        {activity.length === 0 ? (
          <div className="px-5 py-12 text-center text-sm text-[var(--color-muted)]">
            {loading ? t('common.loading') : t('dash.noEntries', { period: periodWord })}
          </div>
        ) : (
          <ul className="divide-y divide-[var(--color-border)]">
            {activity.map((row) => (
              <ListRow key={row.id}>
                <IconBadge
                  tone={
                    row.kind === 'income' ? 'money-in' : row.kind === 'expense' ? 'money-out' : 'muted'
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
                  <div className="text-xs text-[var(--color-muted)] tabular-nums">
                    {row.date}
                    <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                    <span>{t(`kind.${row.kind}`)}</span>
                  </div>
                </div>
                <AmountPill
                  tone={row.kind === 'income' ? 'in' : row.kind === 'expense' ? 'out' : 'neutral'}
                >
                  {money(row.signedMinor, row.kind === 'income' || row.kind === 'expense')}
                </AmountPill>
              </ListRow>
            ))}
          </ul>
        )}
      </Panel>
    </div>
  )
}
