import type {
  BalanceSheet,
  CashFlowBucket,
  CashFlowSeries,
  DashboardSummary,
  PnL,
  PostedEntryView,
  RegisterLine,
  ReportLine,
  TrialBalance,
  AccountType,
} from '../../src/lib/api'
import type { DemoLedger } from './ledger'

type Range = { from: string | null; to: string | null }

function inRange(date: string, { from, to }: Range): boolean {
  return (from === null || date >= from) && (to === null || date <= to)
}

/** Debit-normal for assets and expenses, credit-normal for the rest. */
function normalSign(type: AccountType): 1 | -1 {
  return type === 'asset' || type === 'expense' ? 1 : -1
}

function linesFor(l: DemoLedger, range: Range) {
  return l.entries
    .filter((v) => inRange(v.entry.entry_date, range))
    .flatMap((v) => v.lines.map((line) => ({ line, date: v.entry.entry_date, view: v })))
}

function reportLines(l: DemoLedger, range: Range, types: AccountType[]): ReportLine[] {
  const rows: ReportLine[] = []

  for (const account of l.accounts) {
    if (!types.includes(account.account_type)) continue

    const own = linesFor(l, range).filter((x) => x.line.account_id === account.id)
    const debit = own.reduce((s, x) => s + x.line.debit.amount_minor, 0)
    const credit = own.reduce((s, x) => s + x.line.credit.amount_minor, 0)
    if (debit === 0 && credit === 0) continue

    rows.push({
      code: account.code,
      name: account.name,
      account_type: account.account_type,
      debit_minor: debit,
      credit_minor: credit,
      balance_minor: normalSign(account.account_type) * (debit - credit),
    })
  }

  return rows
}

function sum(rows: ReportLine[]): number {
  return rows.reduce((s, r) => s + r.balance_minor, 0)
}

export function entryList(
  l: DemoLedger,
  opts: { from: string | null; to: string | null; search: string | null; accountId: string | null },
): PostedEntryView[] {
  const needle = opts.search?.trim().toLowerCase() ?? ''

  return l.entries
    .filter((v) => inRange(v.entry.entry_date, opts))
    .filter((v) => needle === '' || v.entry.description.toLowerCase().includes(needle))
    .filter((v) => opts.accountId === null || v.lines.some((x) => x.account_id === opts.accountId))
    .sort((a, b) => b.entry.entry_date.localeCompare(a.entry.entry_date))
}

export function pnl(l: DemoLedger, from: string, to: string): PnL {
  const income = reportLines(l, { from, to }, ['income'])
  const expenses = reportLines(l, { from, to }, ['expense'])
  const total_income = sum(income)
  const total_expenses = sum(expenses)

  return {
    entity_id: l.entity.id,
    from,
    to,
    income,
    expenses,
    total_income,
    total_expenses,
    net_income: total_income - total_expenses,
  }
}

export function trialBalance(l: DemoLedger, asOf: string): TrialBalance {
  const lines = reportLines(l, { from: null, to: asOf }, ['asset', 'liability', 'equity', 'income', 'expense'])

  return {
    entity_id: l.entity.id,
    as_of: asOf,
    lines,
    total_debits: lines.reduce((s, r) => s + r.debit_minor, 0),
    total_credits: lines.reduce((s, r) => s + r.credit_minor, 0),
  }
}

export function balanceSheet(l: DemoLedger, asOf: string): BalanceSheet {
  const range = { from: null, to: asOf }
  const assets = reportLines(l, range, ['asset'])
  const liabilities = reportLines(l, range, ['liability'])
  const equity = reportLines(l, range, ['equity'])
  const earnings = sum(reportLines(l, range, ['income'])) - sum(reportLines(l, range, ['expense']))

  // Rust folds undistributed earnings into equity; mirror that so the sheet balances.
  equity.push({
    code: '3900',
    // The English name Rust sends for this row; the app words it from `synthetic`.
    name: 'Net Income (current period)',
    account_type: 'equity',
    debit_minor: 0,
    credit_minor: earnings,
    balance_minor: earnings,
    // Rust marks this computed row, and the app words it from the marker.
    synthetic: 'net_income',
  })

  const total_assets = sum(assets)
  const total_liabilities = sum(liabilities)
  const total_equity = sum(equity)

  return {
    entity_id: l.entity.id,
    as_of: asOf,
    assets: { lines: assets, total: total_assets },
    liabilities: { lines: liabilities, total: total_liabilities },
    equity: { lines: equity, total: total_equity },
    total_assets,
    total_liabilities_equity: total_liabilities + total_equity,
  }
}

export function accountBalance(l: DemoLedger, accountId: string, asOf: string): number {
  const account = l.accounts.find((a) => a.id === accountId)
  if (!account) return 0

  const own = linesFor(l, { from: null, to: asOf }).filter((x) => x.line.account_id === accountId)
  const net = own.reduce((s, x) => s + x.line.debit.amount_minor - x.line.credit.amount_minor, 0)

  return normalSign(account.account_type) * net
}

export function accountRegister(
  l: DemoLedger,
  accountId: string,
  from: string | null,
  to: string | null,
): RegisterLine[] {
  const account = l.accounts.find((a) => a.id === accountId)
  if (!account) return []

  const sign = normalSign(account.account_type)

  // Seed running balance with the account's balance from all entries strictly before 'from'.
  let running = 0
  if (from !== null) {
    const priorDate = new Date(`${from}T12:00:00`)
    priorDate.setDate(priorDate.getDate() - 1)
    const priorDateStr = iso(priorDate)
    running = accountBalance(l, accountId, priorDateStr)
  }

  return linesFor(l, { from, to })
    .filter((x) => x.line.account_id === accountId)
    .sort((a, b) => a.date.localeCompare(b.date))
    .map((x) => {
      running += sign * (x.line.debit.amount_minor - x.line.credit.amount_minor)

      return {
        entry_id: x.view.entry.id,
        entry_date: x.date,
        description: x.view.entry.description,
        debit_minor: x.line.debit.amount_minor,
        credit_minor: x.line.credit.amount_minor,
        balance_minor: running,
        hidden: false,
      }
    })
}

export function previousWindow(from: string, to: string): { from: string; to: string } {
  const fromDate = new Date(`${from}T12:00:00`)
  const toDate = new Date(`${to}T12:00:00`)

  const prevEnd = new Date(fromDate.getTime() - 86_400_000)

  // Check if this is a whole calendar month: from is 1st of month and to is last day of that month.
  // (i.e., next day after 'to' is the 1st of the next month)
  const nextDay = new Date(toDate.getTime() + 86_400_000)
  const isWholeMonth = fromDate.getDate() === 1 && nextDay.getDate() === 1

  if (isWholeMonth) {
    // Count how many months span from 'from' to 'to'.
    const fromMonthIndex = fromDate.getFullYear() * 12 + fromDate.getMonth()
    const toMonthIndex = toDate.getFullYear() * 12 + toDate.getMonth()
    const months = toMonthIndex - fromMonthIndex + 1

    // Go back that many months from 'from' to get the start of the prior window.
    const prevStart = new Date(fromDate)
    prevStart.setMonth(prevStart.getMonth() - months)

    return { from: iso(prevStart), to: iso(prevEnd) }
  }

  // Otherwise, step back by the same number of days.
  const days = Math.round((toDate.getTime() - fromDate.getTime()) / 86_400_000) + 1
  const prevStart = new Date(prevEnd.getTime() - (days - 1) * 86_400_000)

  return { from: iso(prevStart), to: iso(prevEnd) }
}

function iso(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')

  return `${d.getFullYear()}-${m}-${day}`
}

export function dashboardSummary(
  l: DemoLedger,
  from: string,
  to: string,
  assetsAsOf: string,
): DashboardSummary {
  const p = pnl(l, from, to)
  const prev = previousWindow(from, to)
  const prevNet = pnl(l, prev.from, prev.to).net_income
  const top = [...p.expenses].sort((a, b) => b.balance_minor - a.balance_minor)[0]
  const cashLike = l.accounts
    .filter((a) => a.code === '1000' || a.code === '1020')
    .reduce((s, a) => s + accountBalance(l, a.id, assetsAsOf), 0)
  const bps = (part: number, whole: number) => Math.round((part / whole) * 10_000)

  return {
    entity_id: l.entity.id,
    base_currency: l.entity.base_currency,
    cash_like_assets: cashLike,
    income: p.total_income,
    expenses: p.total_expenses,
    net_income: p.net_income,
    recent_entry_count: entryList(l, { from, to, search: null, accountId: null }).length,
    savings_rate_bps: p.total_income > 0 ? bps(p.net_income, p.total_income) : null,
    spend_ratio_bps: p.total_income > 0 ? bps(p.total_expenses, p.total_income) : null,
    top_expense: top
      ? {
          code: top.code,
          name: top.name,
          amount_minor: top.balance_minor,
          share_bps: bps(top.balance_minor, p.total_expenses),
        }
      : null,
    net_vs_previous_bps: prevNet !== 0 ? bps(p.net_income - prevNet, Math.abs(prevNet)) : null,
  }
}

function eachDay(from: string, to: string): string[] {
  const days: string[] = []

  for (let d = new Date(`${from}T12:00:00`); iso(d) <= to; d = new Date(d.getTime() + 86_400_000)) {
    days.push(iso(d))
  }

  return days
}

function monthEnd(month: string): string {
  const [y, m] = month.split('-').map(Number)

  return iso(new Date(y, m, 0, 12))
}

export function cashFlowSeries(l: DemoLedger, from: string | null, to: string | null): CashFlowSeries {
  const dates = l.entries.map((v) => v.entry.entry_date).sort()
  const start = from ?? dates[0]
  const end = to ?? dates.at(-1)!
  const days = eachDay(start, end)
  const granularity = days.length > 62 ? 'month' : 'day'

  const windows =
    granularity === 'day'
      ? days.map((d) => ({ start: d, end: d }))
      : [...new Set(days.map((d) => d.slice(0, 7)))].map((m) => ({
          start: `${m}-01` < start ? start : `${m}-01`,
          end: monthEnd(m) > end ? end : monthEnd(m),
        }))

  let cumIn = 0
  let cumOut = 0

  const buckets: CashFlowBucket[] = windows.map((w) => {
    const p = pnl(l, w.start, w.end)
    cumIn += p.total_income
    cumOut += p.total_expenses

    return {
      start: w.start,
      end: w.end,
      income_minor: p.total_income,
      expenses_minor: p.total_expenses,
      cumulative_income_minor: cumIn,
      cumulative_expenses_minor: cumOut,
    }
  })

  return {
    entity_id: l.entity.id,
    from: start,
    to: end,
    granularity,
    total_income_minor: cumIn,
    total_expenses_minor: cumOut,
    net_minor: cumIn - cumOut,
    buckets,
  }
}
