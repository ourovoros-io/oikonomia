/** @vitest-environment node */

import { describe, expect, test } from 'vitest'
import {
  billStatusForKind,
  cadenceLabelKey,
  dayOfMonthForCadence,
  firstOnOrAfterMonthDay,
  firstOnOrAfterWeekday,
  formCadenceLabelKey,
  isRecurringCadence,
  isRecurringKind,
  kindBadgeTone,
  kindLabelKey,
  maxDayOfMonth,
  monthAndDayOfIso,
  recurringAccountIds,
  weekdayOfIso,
} from './recurring'

describe('recurring cadence and kind guards', () => {
  test('accepts only the closed cadence set', () => {
    expect(isRecurringCadence('monthly')).toBe(true)
    expect(isRecurringCadence('weekly')).toBe(true)
    expect(isRecurringCadence('yearly')).toBe(true)
    expect(isRecurringCadence('daily')).toBe(false)
    expect(isRecurringCadence('Monthly')).toBe(false)
  })

  test('accepts only journal entry kinds', () => {
    expect(isRecurringKind('expense')).toBe(true)
    expect(isRecurringKind('income')).toBe(true)
    expect(isRecurringKind('bill')).toBe(true)
    expect(isRecurringKind('transfer')).toBe(true)
    expect(isRecurringKind('other')).toBe(false)
  })
})

describe('dayOfMonthForCadence', () => {
  test('keeps a valid day only for monthly', () => {
    expect(dayOfMonthForCadence('monthly', 1)).toBe(1)
    expect(dayOfMonthForCadence('monthly', 31)).toBe(31)
    expect(dayOfMonthForCadence('weekly', 1)).toBeNull()
    expect(dayOfMonthForCadence('yearly', 15)).toBeNull()
  })

  test('rejects out-of-range and non-integer days', () => {
    expect(dayOfMonthForCadence('monthly', 0)).toBeNull()
    expect(dayOfMonthForCadence('monthly', 32)).toBeNull()
    expect(dayOfMonthForCadence('monthly', 1.5)).toBeNull()
    expect(dayOfMonthForCadence('monthly', null)).toBeNull()
  })
})

describe('recurringAccountIds', () => {
  const pick = { categoryId: 'exp1', walletId: 'w1', fromId: 'a1', toId: 'a2' }

  test('expense and bill use category + wallet and drop transfer legs', () => {
    expect(recurringAccountIds('expense', pick)).toEqual({
      category_account_id: 'exp1',
      wallet_account_id: 'w1',
      payable_account_id: null,
      from_account_id: null,
      to_account_id: null,
    })
    expect(recurringAccountIds('bill', pick)).toEqual({
      category_account_id: 'exp1',
      wallet_account_id: 'w1',
      payable_account_id: null,
      from_account_id: null,
      to_account_id: null,
    })
  })

  test('transfer uses from/to and drops category/wallet', () => {
    expect(recurringAccountIds('transfer', pick)).toEqual({
      category_account_id: null,
      wallet_account_id: null,
      payable_account_id: null,
      from_account_id: 'a1',
      to_account_id: 'a2',
    })
  })
})

describe('billStatusForKind', () => {
  test('bills default to paid; other kinds send null', () => {
    expect(billStatusForKind('bill')).toBe('paid')
    expect(billStatusForKind('expense')).toBeNull()
    expect(billStatusForKind('income')).toBeNull()
    expect(billStatusForKind('transfer')).toBeNull()
  })
})

describe('kindBadgeTone', () => {
  test('maps journal kinds onto IconBadge tones from the Ledger, never status', () => {
    expect(kindBadgeTone('income')).toBe('money-in')
    expect(kindBadgeTone('expense')).toBe('money-out')
    expect(kindBadgeTone('bill')).toBe('money-out')
    expect(kindBadgeTone('transfer')).toBe('muted')
  })
})

describe('label keys', () => {
  test('kind labels reuse tx.form.kind.*', () => {
    expect(kindLabelKey('expense')).toBe('tx.form.kind.expense')
    expect(kindLabelKey('income')).toBe('tx.form.kind.income')
    expect(kindLabelKey('bill')).toBe('tx.form.kind.bill')
    expect(kindLabelKey('transfer')).toBe('tx.form.kind.transfer')
  })

  test('list cadence keys stay under recurring.cadence.*', () => {
    expect(cadenceLabelKey('monthly')).toBe('recurring.cadence.monthly')
    expect(cadenceLabelKey('weekly')).toBe('recurring.cadence.weekly')
    expect(cadenceLabelKey('yearly')).toBe('recurring.cadence.yearly')
  })

  test('form cadence keys stay under recurring.form.cadence*', () => {
    expect(formCadenceLabelKey('monthly')).toBe('recurring.form.cadenceMonthly')
    expect(formCadenceLabelKey('weekly')).toBe('recurring.form.cadenceWeekly')
    expect(formCadenceLabelKey('yearly')).toBe('recurring.form.cadenceYearly')
  })
})

describe('recurring start dates', () => {
  test('weekday: the same day counts, otherwise the next one ahead', () => {
    // 2026-10-08 is a Thursday.
    expect(firstOnOrAfterWeekday('2026-10-08', 4)).toBe('2026-10-08')
    expect(firstOnOrAfterWeekday('2026-10-08', 5)).toBe('2026-10-09')
    expect(firstOnOrAfterWeekday('2026-10-08', 3)).toBe('2026-10-14')
    expect(firstOnOrAfterWeekday('2026-12-31', 1)).toBe('2027-01-04')
  })

  test('weekday and month-day are read back from an ISO date', () => {
    expect(weekdayOfIso('2026-10-08')).toBe(4)
    expect(monthAndDayOfIso('2026-10-08')).toEqual({ month: 10, day: 8 })
  })

  test('month and day: this year if still ahead, otherwise next year', () => {
    expect(firstOnOrAfterMonthDay('2026-10-08', 12, 25)).toBe('2026-12-25')
    expect(firstOnOrAfterMonthDay('2026-10-08', 10, 8)).toBe('2026-10-08')
    expect(firstOnOrAfterMonthDay('2026-10-08', 3, 1)).toBe('2027-03-01')
  })

  test('February 29 waits for a leap year instead of overflowing to March', () => {
    expect(firstOnOrAfterMonthDay('2026-10-08', 2, 29)).toBe('2028-02-29')
  })

  test('a month offers at most its own length, February as in a leap year', () => {
    expect(maxDayOfMonth(2)).toBe(29)
    expect(maxDayOfMonth(4)).toBe(30)
    expect(maxDayOfMonth(12)).toBe(31)
  })
})
