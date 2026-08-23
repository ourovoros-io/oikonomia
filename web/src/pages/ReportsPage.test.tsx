/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { Entity, PnL, ReportLine } from '../lib/api'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      reportPnl: vi.fn(),
      reportPnlExport: vi.fn(),
      reportBalanceSheet: vi.fn(),
      reportTrialBalance: vi.fn(),
      reportExportPdf: vi.fn(),
    },
  }
})

vi.mock('../lib/expensePdf', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/expensePdf')>()
  return {
    ...actual,
    buildExpensePdfBytes: vi.fn(async () => new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x2d])),
  }
})

import { api, todayISO, yearStartISO } from '../lib/api'
import { buildExpensePdfBytes, suggestedExpensePdfName } from '../lib/expensePdf'
import { resetI18nForTests, setLocale } from '../lib/i18n'
import { ReportsPage } from './ReportsPage'

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

function expense(
  over: Partial<ReportLine> & Pick<ReportLine, 'code' | 'name' | 'balance_minor'>,
): ReportLine {
  return {
    account_type: 'expense',
    debit_minor: over.balance_minor,
    credit_minor: 0,
    ...over,
  }
}

function pnl(over: Partial<PnL> = {}): PnL {
  return {
    entity_id: 'e1',
    from: '2026-08-01',
    to: '2026-08-31',
    income: [],
    expenses: [expense({ code: '6100', name: 'Rent', balance_minor: 850_00 })],
    total_income: 0,
    total_expenses: 850_00,
    net_income: -850_00,
    ...over,
  }
}

beforeEach(() => {
  resetI18nForTests()
  vi.mocked(api.reportPnl).mockReset().mockResolvedValue(pnl())
  vi.mocked(api.reportPnlExport).mockReset().mockResolvedValue(pnl())
  vi.mocked(api.reportBalanceSheet).mockReset().mockResolvedValue({
    entity_id: 'e1',
    as_of: '2026-08-23',
    assets: { title: 'assets', lines: [], total: 0 },
    liabilities: { title: 'liabilities', lines: [], total: 0 },
    equity: { title: 'equity', lines: [], total: 0 },
    total_assets: 0,
    total_liabilities_equity: 0,
  })
  vi.mocked(api.reportTrialBalance).mockReset().mockResolvedValue({
    entity_id: 'e1',
    as_of: '2026-08-23',
    lines: [],
    total_debits: 0,
    total_credits: 0,
  })
  vi.mocked(api.reportExportPdf).mockReset().mockResolvedValue('/tmp/report.pdf')
  vi.mocked(buildExpensePdfBytes).mockClear()
})

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

describe('ReportsPage Export PDF', () => {
  test('toolbar button is on P&L only and uses reports.exportPdf', async () => {
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Export PDF' })).toBeTruthy()
    })
    expect(screen.getByRole('button', { name: 'Refresh' })).toBeTruthy()

    await userEvent.click(screen.getByRole('button', { name: 'Balance sheet' }))
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Export PDF' })).toBeNull()
    })

    await userEvent.click(screen.getByRole('button', { name: 'Trial balance' }))
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Export PDF' })).toBeNull()
    })

    await userEvent.click(screen.getByRole('button', { name: 'P&L' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Export PDF' })).toBeTruthy()
    })
  })

  test('export path sends PDF bytes and the suggested filename', async () => {
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Export PDF' })).toBeTruthy()
    })
    await waitFor(() => {
      expect(api.reportPnl).toHaveBeenCalled()
    })
    const inAppCalls = vi.mocked(api.reportPnl).mock.calls.length
    await userEvent.click(screen.getByRole('button', { name: 'Export PDF' }))
    await waitFor(() => {
      expect(api.reportExportPdf).toHaveBeenCalledTimes(1)
    })
    expect(api.reportPnl).toHaveBeenCalledTimes(inAppCalls)
    expect(buildExpensePdfBytes).toHaveBeenCalledWith({
      entityName: 'Personal',
      currency: 'EUR',
      from: '2026-08-01',
      to: '2026-08-31',
      expenses: [expense({ code: '6100', name: 'Rent', balance_minor: 850_00 })],
    })
    expect(api.reportPnlExport).toHaveBeenCalledWith('e1', yearStartISO(), todayISO())
    expect(api.reportExportPdf).toHaveBeenCalledWith({
      bytesBase64: expect.any(String),
      suggestedName: suggestedExpensePdfName('2026-08-01', '2026-08-31'),
    })
    const payload = vi.mocked(api.reportExportPdf).mock.calls[0]?.[0]
    expect(payload?.bytesBase64.length).toBeGreaterThan(0)
  })

  test('cancel / null from the save dialog is not an error', async () => {
    vi.mocked(api.reportExportPdf).mockResolvedValue(null)
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Export PDF' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Export PDF' }))
    await waitFor(() => {
      expect(api.reportExportPdf).toHaveBeenCalled()
    })
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('PDF uses export P&L and leaves in-app Hidden totals alone', async () => {
    vi.mocked(api.reportPnl).mockResolvedValue(
      pnl({
        expenses: [
          expense({ code: '5100', name: 'Food', balance_minor: 2_500 }),
          expense({ code: '5200', name: 'Secret trip', balance_minor: 1_000 }),
        ],
        total_expenses: 3_500,
        net_income: -3_500,
      }),
    )
    vi.mocked(api.reportPnlExport).mockResolvedValue(
      pnl({
        expenses: [expense({ code: '5100', name: 'Food', balance_minor: 2_500 })],
        total_expenses: 2_500,
        net_income: -2_500,
      }),
    )
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getAllByText('Secret trip').length).toBeGreaterThan(0)
    })
    await userEvent.click(screen.getByRole('button', { name: 'Export PDF' }))
    await waitFor(() => {
      expect(api.reportPnlExport).toHaveBeenCalledWith('e1', yearStartISO(), todayISO())
    })
    expect(buildExpensePdfBytes).toHaveBeenCalledWith(
      expect.objectContaining({
        expenses: [expense({ code: '5100', name: 'Food', balance_minor: 2_500 })],
      }),
    )
    expect(screen.getAllByText('Secret trip').length).toBeGreaterThan(0)
  })

  test('empty period still builds and exports', async () => {
    vi.mocked(api.reportPnlExport).mockResolvedValue(
      pnl({ expenses: [], total_expenses: 0, net_income: 0 }),
    )
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Export PDF' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Export PDF' }))
    await waitFor(() => {
      expect(api.reportExportPdf).toHaveBeenCalledTimes(1)
    })
    expect(buildExpensePdfBytes).toHaveBeenCalledWith(
      expect.objectContaining({ expenses: [] }),
    )
  })

  test('EL toolbar uses Writer reports.exportPdf', async () => {
    setLocale('el')
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Εξαγωγή PDF' })).toBeTruthy()
    })
  })
})
