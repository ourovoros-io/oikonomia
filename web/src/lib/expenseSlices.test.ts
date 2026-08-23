import { afterEach, describe, expect, test } from 'vitest'
import type { ReportLine } from './api'
import { MAX_SLICES, VIZ_DARK_HEX, buildSlices, vizHex } from './expenseSlices'
import { resetI18nForTests } from './i18n'

afterEach(() => {
  resetI18nForTests()
})

function line(over: Partial<ReportLine> & Pick<ReportLine, 'code' | 'name' | 'balance_minor'>): ReportLine {
  return {
    account_type: 'expense',
    debit_minor: over.balance_minor,
    credit_minor: 0,
    ...over,
  }
}

describe('buildSlices', () => {
  test('folds ranks after the top 6 into Other and colors top 6 by code order', () => {
    const lines = [
      line({ code: '6100', name: 'Rent', balance_minor: 850_00 }),
      line({ code: '6200', name: 'Groceries', balance_minor: 420_00 }),
      line({ code: '6300', name: 'Dining', balance_minor: 285_00 }),
      line({ code: '6400', name: 'Transport', balance_minor: 210_00 }),
      line({ code: '6500', name: 'Utilities', balance_minor: 165_00 }),
      line({ code: '6600', name: 'Subscriptions', balance_minor: 92_00 }),
      line({ code: '6700', name: 'Health', balance_minor: 42_00 }),
      line({ code: '6800', name: 'Clothing', balance_minor: 24_00 }),
      line({ code: '6900', name: 'Misc', balance_minor: 12_00 }),
    ]
    const { slices, total } = buildSlices(lines)
    expect(MAX_SLICES).toBe(6)
    expect(total).toBe(2100_00)
    expect(slices).toHaveLength(7)
    expect(slices.slice(0, 6).map((s) => s.name)).toEqual([
      'Rent',
      'Groceries',
      'Dining',
      'Transport',
      'Utilities',
      'Subscriptions',
    ])
    expect(slices[6]).toMatchObject({
      name: 'Other (3 categories)',
      amount: 78_00,
      slot: 'other',
    })
    expect(vizHex(slices[6].slot)).toBe(VIZ_DARK_HEX.other)

    const topSlots = slices.slice(0, 6).map((s) => s.slot)
    expect(topSlots).toEqual([1, 2, 3, 4, 5, 6])
  })

  test('assigns viz slots by account code, not amount rank', () => {
    const { slices } = buildSlices([
      line({ code: '6900', name: 'Zebra', balance_minor: 900_00 }),
      line({ code: '6100', name: 'Alpha', balance_minor: 100_00 }),
    ])
    expect(slices[0]).toMatchObject({ name: 'Zebra', slot: 2 })
    expect(slices[1]).toMatchObject({ name: 'Alpha', slot: 1 })
    expect(vizHex(1)).toBe('#3987e5')
    expect(vizHex(2)).toBe('#d95926')
  })

  test('drops zero and negative balances', () => {
    const { slices, total } = buildSlices([
      line({ code: '6100', name: 'Rent', balance_minor: 100_00 }),
      line({ code: '6200', name: 'Void', balance_minor: 0 }),
      line({ code: '6300', name: 'Credit', balance_minor: -20_00 }),
    ])
    expect(total).toBe(100_00)
    expect(slices).toHaveLength(1)
    expect(slices[0].name).toBe('Rent')
  })

  test('empty period has no slices', () => {
    expect(buildSlices([])).toEqual({ slices: [], total: 0 })
  })
})
