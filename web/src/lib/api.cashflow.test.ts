/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

import { invoke } from '@tauri-apps/api/core'
import { api, type CashFlowSeries, type DashboardSummary } from './api'

const series: CashFlowSeries = {
  entity_id: 'e1',
  from: '2026-08-01',
  to: '2026-08-02',
  granularity: 'day',
  total_income_minor: 500,
  total_expenses_minor: 120,
  net_minor: 380,
  buckets: [
    {
      start: '2026-08-01',
      end: '2026-08-01',
      income_minor: 500,
      expenses_minor: 0,
      cumulative_income_minor: 500,
      cumulative_expenses_minor: 0,
    },
    {
      start: '2026-08-02',
      end: '2026-08-02',
      income_minor: 0,
      expenses_minor: 120,
      cumulative_income_minor: 500,
      cumulative_expenses_minor: 120,
    },
  ],
}

beforeEach(() => {
  Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
  vi.mocked(invoke).mockReset()
})

afterEach(() => {
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('cash flow over IPC', () => {
  test('cashFlowSeries passes open bounds to Rust as null', async () => {
    vi.mocked(invoke).mockResolvedValue(series)

    await expect(api.cashFlowSeries('e1', null, '2026-08-31')).resolves.toEqual(series)
    expect(invoke).toHaveBeenCalledWith('cash_flow_series_cmd', {
      entityId: 'e1',
      from: null,
      to: '2026-08-31',
    })
  })

  test('the dashboard summary carries the arc metrics, empty ones as null', async () => {
    const summary: DashboardSummary = {
      entity_id: 'e1',
      base_currency: 'EUR',
      cash_like_assets: 0,
      income: 0,
      expenses: 2500,
      net_income: -2500,
      recent_entry_count: 1,
      savings_rate_bps: null,
      spend_ratio_bps: null,
      top_expense: { code: '5100', name: 'Food', amount_minor: 2500, share_bps: 10000 },
      net_vs_previous_bps: null,
    }
    vi.mocked(invoke).mockResolvedValue(summary)

    const result = await api.dashboardSummary('e1', '2026-08-01', '2026-08-31', '2026-08-15')
    expect(result.savings_rate_bps).toBeNull()
    expect(result.top_expense?.share_bps).toBe(10000)
  })
})
