/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

import { invoke } from '@tauri-apps/api/core'
import { api, type RecurringPostResult, type RecurringTemplate } from './api'

const row: RecurringTemplate = {
  id: 'r1',
  entity_id: 'e1',
  name: 'Rent',
  kind: 'expense',
  bill_status: null,
  amount_minor: 85000,
  cadence: 'monthly',
  day_of_month: 1,
  category_account_id: 'exp1',
  wallet_account_id: 'w1',
  payable_account_id: null,
  from_account_id: null,
  to_account_id: null,
  memo: null,
  next_date: '2026-08-01',
  due: true,
}

const posted: RecurringPostResult = {
  entry: {
    entry: {
      id: 'j1',
      entity_id: 'e1',
      entry_date: '2026-08-01',
      description: 'Rent',
      reference: null,
      status: 'posted',
      hidden: false,
    },
    lines: [],
    is_voided: false,
  },
  template: { ...row, next_date: '2026-09-01', due: false },
}

beforeEach(() => {
  Object.defineProperty(window, '__TAURI_INTERNALS__', {
    value: {},
    configurable: true,
  })
  vi.mocked(invoke).mockReset()
})

afterEach(() => {
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('recurring IPC shape', () => {
  test('recurringList invokes recurring_list with entityId', async () => {
    vi.mocked(invoke).mockResolvedValue([row])
    await expect(api.recurringList('e1')).resolves.toEqual([row])
    expect(invoke).toHaveBeenCalledWith('recurring_list', { entityId: 'e1' })
  })

  test('recurringCreate wraps the input', async () => {
    vi.mocked(invoke).mockResolvedValue(row)
    const input = {
      entity_id: 'e1',
      name: 'Rent',
      kind: 'expense' as const,
      bill_status: null,
      amount_minor: 85000,
      cadence: 'monthly' as const,
      day_of_month: 1,
      category_account_id: 'exp1',
      wallet_account_id: 'w1',
      payable_account_id: null,
      from_account_id: null,
      to_account_id: null,
      memo: null,
      next_date: '2026-08-01',
    }
    await api.recurringCreate(input)
    expect(invoke).toHaveBeenCalledWith('recurring_create', { input })
  })

  test('recurringUpdate sends id inside input', async () => {
    vi.mocked(invoke).mockResolvedValue(row)
    const input = {
      id: 'r1',
      name: 'Rent',
      kind: 'expense' as const,
      bill_status: null,
      amount_minor: 85000,
      cadence: 'monthly' as const,
      day_of_month: 1,
      category_account_id: 'exp1',
      wallet_account_id: 'w1',
      payable_account_id: null,
      from_account_id: null,
      to_account_id: null,
      memo: null,
      next_date: '2026-08-01',
    }
    await api.recurringUpdate(input)
    expect(invoke).toHaveBeenCalledWith('recurring_update', { input })
  })

  test('recurringPost sends entryDate and amountMinor overrides', async () => {
    vi.mocked(invoke).mockResolvedValue(posted)
    await api.recurringPost('r1', { entry_date: '2026-08-03', amount_minor: 90000 })
    expect(invoke).toHaveBeenCalledWith('recurring_post', {
      id: 'r1',
      entryDate: '2026-08-03',
      amountMinor: 90000,
    })
  })

  test('recurringPost omits overrides as null', async () => {
    vi.mocked(invoke).mockResolvedValue(posted)
    await api.recurringPost('r1')
    expect(invoke).toHaveBeenCalledWith('recurring_post', {
      id: 'r1',
      entryDate: null,
      amountMinor: null,
    })
  })
})
