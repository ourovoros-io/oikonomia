import type { RecurringCadence, RecurringKind, RecurringTemplateInput } from './api'

const CADENCES = ['monthly', 'weekly', 'yearly'] as const satisfies readonly RecurringCadence[]
const KINDS = ['expense', 'income', 'bill', 'transfer'] as const satisfies readonly RecurringKind[]

export function isRecurringCadence(value: string): value is RecurringCadence {
  return (CADENCES as readonly string[]).includes(value)
}

export function isRecurringKind(value: string): value is RecurringKind {
  return (KINDS as readonly string[]).includes(value)
}

/** Weekly/Yearly must not carry a day-of-month; only Monthly does. */
export function dayOfMonthForCadence(
  cadence: RecurringCadence,
  day: number | null,
): number | null {
  if (cadence !== 'monthly') return null
  if (day == null || !Number.isInteger(day) || day < 1 || day > 31) return null
  return day
}

export type RecurringAccountPick = {
  categoryId: string
  walletId: string
  fromId: string
  toId: string
}

/**
 * Kind decides which account ids are meaningful.
 * Transfer uses from/to; other kinds use category + wallet (From account).
 */
export function recurringAccountIds(
  kind: RecurringKind,
  pick: RecurringAccountPick,
): Pick<
  RecurringTemplateInput,
  | 'category_account_id'
  | 'wallet_account_id'
  | 'payable_account_id'
  | 'from_account_id'
  | 'to_account_id'
> {
  if (kind === 'transfer') {
    return {
      category_account_id: null,
      wallet_account_id: null,
      payable_account_id: null,
      from_account_id: pick.fromId || null,
      to_account_id: pick.toId || null,
    }
  }
  return {
    category_account_id: pick.categoryId || null,
    wallet_account_id: pick.walletId || null,
    payable_account_id: null,
    from_account_id: null,
    to_account_id: null,
  }
}

export function kindLabelKey(kind: RecurringKind): `tx.form.kind.${RecurringKind}` {
  return `tx.form.kind.${kind}`
}

/**
 * List/sheet kind tiles follow Transactions: money identity uses the Ledger,
 * never status, so income wears money-in and expense/bill wear money-out;
 * transfer stays neutral.
 */
export function kindBadgeTone(kind: RecurringKind): 'money-in' | 'money-out' | 'muted' {
  switch (kind) {
    case 'income':
      return 'money-in'
    case 'expense':
    case 'bill':
      return 'money-out'
    case 'transfer':
      return 'muted'
  }
}

export function cadenceLabelKey(
  cadence: RecurringCadence,
): 'recurring.cadence.monthly' | 'recurring.cadence.weekly' | 'recurring.cadence.yearly' {
  switch (cadence) {
    case 'weekly':
      return 'recurring.cadence.weekly'
    case 'yearly':
      return 'recurring.cadence.yearly'
    case 'monthly':
      return 'recurring.cadence.monthly'
  }
}

/** Bills require a status; other kinds must send null. Default paid matches the journal form. */
export function billStatusForKind(
  kind: RecurringKind,
): 'paid' | 'unpaid' | 'pay_existing' | null {
  return kind === 'bill' ? 'paid' : null
}

export function formCadenceLabelKey(
  cadence: RecurringCadence,
):
  | 'recurring.form.cadenceMonthly'
  | 'recurring.form.cadenceWeekly'
  | 'recurring.form.cadenceYearly' {
  switch (cadence) {
    case 'weekly':
      return 'recurring.form.cadenceWeekly'
    case 'yearly':
      return 'recurring.form.cadenceYearly'
    case 'monthly':
      return 'recurring.form.cadenceMonthly'
  }
}

const MS_PER_DAY = 86_400_000

function pad(value: number): string {
  return String(value).padStart(2, '0')
}

function toIso(date: Date): string {
  return `${date.getUTCFullYear()}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())}`
}

function parseIso(iso: string): Date {
  const [year, month, day] = iso.split('-').map(Number)
  return new Date(Date.UTC(year, month - 1, day))
}

/** Weekday of an ISO date, 0 for Sunday to 6 for Saturday. */
export function weekdayOfIso(iso: string): number {
  return parseIso(iso).getUTCDay()
}

/** Month (1 to 12) and day of an ISO date. */
export function monthAndDayOfIso(iso: string): { month: number; day: number } {
  const date = parseIso(iso)
  return { month: date.getUTCMonth() + 1, day: date.getUTCDate() }
}

/**
 * The first date on or after `from` that falls on `weekday` (0 for Sunday).
 *
 * A weekly template recurs on the weekday of its next date, so this is how
 * the form turns "every Friday" into the date the template starts on.
 */
export function firstOnOrAfterWeekday(from: string, weekday: number): string {
  const start = parseIso(from)
  const ahead = (weekday - start.getUTCDay() + 7) % 7

  return toIso(new Date(start.getTime() + ahead * MS_PER_DAY))
}

/** The most days `month` can have, counting February as a leap year has it. */
export function maxDayOfMonth(month: number): number {
  return new Date(Date.UTC(2024, month, 0)).getUTCDate()
}

/**
 * The first date on or after `from` that falls on `day` of `month`.
 *
 * A yearly template recurs on the month and day of its next date. February
 * 29 is offered, and lands on the next leap year, so it never starts on the
 * 1st of March by overflow.
 */
export function firstOnOrAfterMonthDay(from: string, month: number, day: number): string {
  const start = parseIso(from)
  const lastYear = start.getUTCFullYear() + 8

  for (let year = start.getUTCFullYear(); year <= lastYear; year += 1) {
    const candidate = new Date(Date.UTC(year, month - 1, day))
    const exists = candidate.getUTCMonth() === month - 1 && candidate.getUTCDate() === day
    if (exists && candidate >= start) return toIso(candidate)
  }

  return from
}
