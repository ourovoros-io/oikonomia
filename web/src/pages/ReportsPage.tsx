import { useEffect, useRef, useState, type ReactNode } from 'react'
import { BarChart3, FileSpreadsheet, FileText, PieChart, RefreshCw, Scale } from 'lucide-react'
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
  buildExpensePdfBytes,
  bytesToBase64,
  pdfExportErrorMessage,
  suggestedExpensePdfName,
} from '../lib/expensePdf'
import { beginExclusive } from '../lib/guards'
import { DateInput } from '../components/DateInput'
import { ExpenseDonut } from '../components/ExpenseDonut'
import {
  Button,
  Card,
  EmptyState,
  ErrorBanner,
  Field,
  PageHeader,
  Panel,
  Segmented,
} from '../components/ui'
import { cn } from '../lib/cn'
import type { CommandError } from '../lib/tauri'
import { t } from '../lib/i18n'
import { useI18n } from '../lib/I18nProvider'

type Props = { entity: Entity | null }
type Tab = 'trial' | 'pnl' | 'bs'

function sectionTitle(title: string): string {
  const key = title.toLowerCase()
  if (key === 'assets') return t('rpt.section.assets')
  if (key === 'liabilities') return t('rpt.section.liabilities')
  if (key === 'equity') return t('rpt.section.equity')
  return title
}

export function ReportsPage({ entity }: Props) {
  const { t } = useI18n()
  const [tab, setTab] = useState<Tab>('pnl')
  const [asOf, setAsOf] = useState(todayISO())
  const [from, setFrom] = useState(yearStartISO())
  const [to, setTo] = useState(todayISO())
  const [error, setError] = useState<string | null>(null)
  const [tb, setTb] = useState<TrialBalance | null>(null)
  const [pnl, setPnl] = useState<PnL | null>(null)
  const [bs, setBs] = useState<BalanceSheet | null>(null)
  const [pdfBusy, setPdfBusy] = useState(false)
  const pdfBusyRef = useRef(false)

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

  async function onExportPdf() {
    if (!entity || tab !== 'pnl') return
    if (!beginExclusive(pdfBusyRef)) return
    setPdfBusy(true)
    setError(null)
    try {
      let data = pnl
      if (!data || data.from !== from || data.to !== to) {
        data = await api.reportPnl(entity.id, from, to)
        setPnl(data)
      }
      const bytes = await buildExpensePdfBytes({
        entityName: entity.name,
        currency: entity.base_currency,
        from: data.from,
        to: data.to,
        expenses: data.expenses,
      })
      await api.reportExportPdf({
        bytesBase64: bytesToBase64(bytes),
        suggestedName: suggestedExpensePdfName(data.from, data.to),
      })
    } catch (err) {
      setError(pdfExportErrorMessage(err as CommandError))
    } finally {
      pdfBusyRef.current = false
      setPdfBusy(false)
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
        title={t('rpt.noBookTitle')}
        body={t('rpt.noBookBody')}
      />
    )
  }

  const ccy = entity.base_currency

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow={t('rpt.eyebrow')}
        title={t('rpt.title')}
        description={t('rpt.description')}
        meta={entity.name}
        actions={
          <Segmented<Tab>
            value={tab}
            onChange={setTab}
            options={[
              { id: 'pnl', label: t('rpt.pnl'), icon: <BarChart3 className="size-3.5" /> },
              { id: 'bs', label: t('rpt.balanceSheet'), icon: <Scale className="size-3.5" /> },
              {
                id: 'trial',
                label: t('rpt.trialBalance'),
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
              <Field label={t('rpt.from')} className="w-44">
                <DateInput value={from} onChange={setFrom} required aria-label={t('rpt.fromDate')} />
              </Field>
              <Field label={t('rpt.to')} className="w-44">
                <DateInput value={to} onChange={setTo} required aria-label={t('rpt.toDate')} />
              </Field>
            </>
          ) : (
            <Field label={t('rpt.asOf')} className="w-44">
              <DateInput value={asOf} onChange={setAsOf} required aria-label={t('rpt.asOfDate')} />
            </Field>
          )}
          <Button variant="secondary" onClick={() => void run()} disabled={pdfBusy}>
            <RefreshCw className="size-4" />
            {t('rpt.refresh')}
          </Button>
          {tab === 'pnl' ? (
            <Button
              variant="secondary"
              busy={pdfBusy}
              disabled={pdfBusy}
              onClick={() => void onExportPdf()}
            >
              <FileText className="size-4" />
              {pdfBusy ? t('reports.pdf.busy') : t('reports.pdf.export')}
            </Button>
          ) : null}
        </div>
      </Card>

      <ErrorBanner message={error} />

      {tab === 'trial' && tb ? <TrialView tb={tb} entityName={entity.name} ccy={ccy} /> : null}
      {tab === 'pnl' && pnl ? <PnlView pnl={pnl} entityName={entity.name} ccy={ccy} /> : null}
      {tab === 'bs' && bs ? <BsView bs={bs} entityName={entity.name} ccy={ccy} /> : null}
    </div>
  )
}

// --- Statement building blocks ---------------------------------------------

function Statement({
  entityName,
  title,
  period,
  ccy,
  children,
}: {
  entityName: string
  title: string
  period: string
  ccy: string
  children: ReactNode
}) {
  return (
    <Card padding="lg" className="mx-auto w-full max-w-2xl">
      <div className="mb-6 border-b border-[var(--color-border)] pb-4 text-center">
        <p className="text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
          {entityName}
        </p>
        <h3 className="mt-1 text-lg font-semibold tracking-tight text-[var(--color-fg)]">
          {title}
        </h3>
        <p className="mt-0.5 text-xs text-[var(--color-muted)]">
          {t('rpt.amountsIn', { period, ccy })}
        </p>
      </div>
      {children}
    </Card>
  )
}

function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <div className="mt-6 mb-1 text-[11px] font-semibold tracking-[0.12em] text-[var(--color-muted)] uppercase first:mt-0">
      {children}
    </div>
  )
}

function LineRow({ line, ccy }: { line: ReportLine; ccy: string }) {
  return (
    <div className="flex items-baseline justify-between gap-4 py-1.5">
      <div className="flex min-w-0 items-baseline gap-2.5">
        <span className="shrink-0 text-xs tabular-nums text-[var(--color-muted)]">{line.code}</span>
        <span className="truncate text-sm text-[var(--color-fg-secondary)]">{line.name}</span>
      </div>
      <span className="shrink-0 text-sm tabular-nums text-[var(--color-fg)]">
        {formatMoney(line.balance_minor, ccy)}
      </span>
    </div>
  )
}

function EmptyLines({ children }: { children: ReactNode }) {
  return <p className="py-1.5 text-sm text-[var(--color-muted)] italic">{children}</p>
}

function TotalRow({
  label,
  amount,
  ccy,
  grand = false,
  tone,
}: {
  label: string
  amount: number
  ccy: string
  /** Grand totals close with a double rule, section totals with a single one. */
  grand?: boolean
  tone?: 'danger' | 'success'
}) {
  return (
    <div
      className={cn(
        'flex items-baseline justify-between gap-4 py-2',
        grand
          ? 'mt-3 border-t-4 border-double border-[var(--color-border-strong)]'
          : 'border-t border-[var(--color-border)]',
      )}
    >
      <span className={cn('text-sm text-[var(--color-fg)]', grand ? 'font-semibold' : 'font-medium')}>
        {label}
      </span>
      <span
        className={cn(
          'shrink-0 text-sm tabular-nums',
          grand ? 'font-semibold' : 'font-medium',
          tone === 'danger'
            ? 'text-[var(--color-danger)]'
            : tone === 'success'
              ? 'text-[var(--color-success)]'
              : 'text-[var(--color-fg)]',
        )}
      >
        {formatMoney(amount, ccy)}
      </span>
    </div>
  )
}

// --- Views ------------------------------------------------------------------

function PnlView({ pnl, entityName, ccy }: { pnl: PnL; entityName: string; ccy: string }) {
  const net = pnl.net_income
  return (
    <div className="space-y-6">
      <Statement
        entityName={entityName}
        title={t('rpt.profitLoss')}
        period={`${formatDate(pnl.from)} – ${formatDate(pnl.to)}`}
        ccy={ccy}
      >
        <SectionLabel>{t('rpt.income')}</SectionLabel>
        {pnl.income.length === 0 ? (
          <EmptyLines>{t('rpt.noIncome')}</EmptyLines>
        ) : (
          pnl.income.map((l) => <LineRow key={l.code + l.name} line={l} ccy={ccy} />)
        )}
        <TotalRow label={t('rpt.totalIncome')} amount={pnl.total_income} ccy={ccy} />

        <SectionLabel>{t('rpt.expenses')}</SectionLabel>
        {pnl.expenses.length === 0 ? (
          <EmptyLines>{t('rpt.noExpenses')}</EmptyLines>
        ) : (
          pnl.expenses.map((l) => <LineRow key={l.code + l.name} line={l} ccy={ccy} />)
        )}
        <TotalRow label={t('rpt.totalExpenses')} amount={pnl.total_expenses} ccy={ccy} />

        <TotalRow
          label={t('rpt.netIncome')}
          amount={net}
          ccy={ccy}
          grand
          tone={net < 0 ? 'danger' : 'success'}
        />
      </Statement>

      <Panel
        title={t('rpt.expenseBreakdown')}
        description={t('rpt.expenseBreakdownDesc')}
        icon={<PieChart className="size-4" />}
      >
        <ExpenseDonut lines={pnl.expenses} ccy={ccy} />
      </Panel>
    </div>
  )
}

function BsView({ bs, entityName, ccy }: { bs: BalanceSheet; entityName: string; ccy: string }) {
  const diff = bs.total_assets - bs.total_liabilities_equity
  return (
    <Statement
      entityName={entityName}
      title={t('rpt.balanceSheet')}
      period={t('rpt.asOfPeriod', { date: formatDate(bs.as_of) })}
      ccy={ccy}
    >
      {([bs.assets, bs.liabilities, bs.equity] as const).map((section) => (
        <div key={section.title}>
          <SectionLabel>{section.title}</SectionLabel>
          {section.lines.length === 0 ? (
            <EmptyLines>
              {t('rpt.noSectionAccounts', { section: sectionTitle(section.title) })}
            </EmptyLines>
          ) : (
            section.lines.map((l) => <LineRow key={l.code + l.name} line={l} ccy={ccy} />)
          )}
          <TotalRow
            label={t('rpt.totalSection', { section: sectionTitle(section.title) })}
            amount={section.total}
            ccy={ccy}
          />
        </div>
      ))}

      <TotalRow label={t('rpt.totalAssets')} amount={bs.total_assets} ccy={ccy} grand />
      <TotalRow
        label={t('rpt.totalLiabEquity')}
        amount={bs.total_liabilities_equity}
        ccy={ccy}
        grand
      />
      <p
        className={cn(
          'mt-3 text-center text-xs',
          diff === 0 ? 'text-[var(--color-muted)]' : 'text-[var(--color-danger)]',
        )}
      >
        {diff === 0
          ? t('rpt.booksBalance')
          : t('rpt.outOfBalance', { amount: formatMoney(diff, ccy) })}
      </p>
    </Statement>
  )
}

function TrialView({ tb, entityName, ccy }: { tb: TrialBalance; entityName: string; ccy: string }) {
  return (
    <Statement
      entityName={entityName}
      title={t('rpt.trialBalance')}
      period={t('rpt.asOfPeriod', { date: formatDate(tb.as_of) })}
      ccy={ccy}
    >
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-[var(--color-border)]">
              <th className="py-2 pr-4 text-left text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
                {t('rpt.account')}
              </th>
              <th className="w-32 py-2 pl-4 text-right text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
                {t('rpt.debit')}
              </th>
              <th className="w-32 py-2 pl-4 text-right text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
                {t('rpt.credit')}
              </th>
            </tr>
          </thead>
          <tbody>
            {tb.lines.map((l) => (
              <tr key={l.code + l.name} className="border-b border-[var(--color-border)]/60">
                <td className="py-2 pr-4">
                  <span className="mr-2.5 text-xs tabular-nums text-[var(--color-muted)]">
                    {l.code}
                  </span>
                  <span className="text-[var(--color-fg-secondary)]">{l.name}</span>
                </td>
                <td className="py-2 pl-4 text-right tabular-nums text-[var(--color-fg)]">
                  {l.debit_minor ? formatMoney(l.debit_minor, ccy) : '—'}
                </td>
                <td className="py-2 pl-4 text-right tabular-nums text-[var(--color-fg)]">
                  {l.credit_minor ? formatMoney(l.credit_minor, ccy) : '—'}
                </td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="border-t-4 border-double border-[var(--color-border-strong)] font-semibold">
              <td className="py-2.5 pr-4 text-[var(--color-fg)]">{t('rpt.total')}</td>
              <td className="py-2.5 pl-4 text-right tabular-nums text-[var(--color-fg)]">
                {formatMoney(tb.total_debits, ccy)}
              </td>
              <td className="py-2.5 pl-4 text-right tabular-nums text-[var(--color-fg)]">
                {formatMoney(tb.total_credits, ccy)}
              </td>
            </tr>
          </tfoot>
        </table>
      </div>
    </Statement>
  )
}
