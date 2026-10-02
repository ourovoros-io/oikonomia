/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { BalanceSheet, Entity, PnL, ReportLine, TrialBalance } from '../lib/api'

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

import { api, monthEndISO, monthStartISO, todayISO } from '../lib/api'
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

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
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
    expect(api.reportPnlExport).toHaveBeenCalledWith('e1', monthStartISO(), monthEndISO())
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
      expect(api.reportPnlExport).toHaveBeenCalledWith('e1', monthStartISO(), monthEndISO())
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

describe('ReportsPage historical dates', () => {
  test('opens on the current month, the same window as the dashboard', async () => {
    render(<ReportsPage entity={entity} />)

    await waitFor(() => {
      expect(api.reportPnl).toHaveBeenCalledWith('e1', monthStartISO(), monthEndISO())
    })
    expect(api.reportPnl).not.toHaveBeenCalledWith('e1', expect.stringMatching(/-01-01$/), expect.anything())
  })

  test('changing From refetches P&L for the new window', async () => {
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(api.reportPnl).toHaveBeenCalledWith('e1', monthStartISO(), monthEndISO())
    })

    const from = screen.getByLabelText('Report from date')
    await user.clear(from)
    await user.type(from, '01/03/2025')
    await user.tab()

    await waitFor(() => {
      expect(api.reportPnl).toHaveBeenCalledWith('e1', '2025-03-01', monthEndISO())
    })
  })

  test('hides the previous period while a new date range is loading', async () => {
    vi.mocked(api.reportPnl)
      .mockResolvedValueOnce(
        pnl({
          from: monthStartISO(),
          to: monthEndISO(),
          expenses: [expense({ code: '6100', name: 'Rent', balance_minor: 850_00 })],
        }),
      )
      .mockImplementation(() => new Promise(() => {}))

    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getAllByText('Rent').length).toBeGreaterThan(0)
    })

    const from = screen.getByLabelText('Report from date')
    await user.clear(from)
    await user.type(from, '01/01/2025')
    await user.tab()

    await waitFor(() => {
      expect(screen.queryAllByText('Rent')).toHaveLength(0)
    })
  })

  test('a completed refetch paints the new window and drops the previous lines', async () => {
    vi.mocked(api.reportPnl)
      .mockResolvedValueOnce(
        pnl({
          from: monthStartISO(),
          to: monthEndISO(),
          expenses: [expense({ code: '6100', name: 'Rent', balance_minor: 850_00 })],
        }),
      )
      .mockResolvedValueOnce(
        pnl({
          from: '2025-01-01',
          to: monthEndISO(),
          expenses: [expense({ code: '5100', name: 'Groceries', balance_minor: 40_00 })],
          total_expenses: 40_00,
          net_income: -40_00,
        }),
      )

    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getAllByText('Rent').length).toBeGreaterThan(0)
    })

    const from = screen.getByLabelText('Report from date')
    await user.clear(from)
    await user.type(from, '01/01/2025')
    await user.tab()

    await waitFor(() => {
      expect(screen.getAllByText('Groceries').length).toBeGreaterThan(0)
    })
    expect(screen.queryAllByText('Rent')).toHaveLength(0)
  })

  test('a late failure from the previous window does not overlay the new statement', async () => {
    const first = deferred<PnL>()
    const second = deferred<PnL>()
    vi.mocked(api.reportPnl)
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise)

    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(api.reportPnl).toHaveBeenCalledTimes(1)
    })

    const from = screen.getByLabelText('Report from date')
    await user.clear(from)
    await user.type(from, '01/01/2025')
    await user.tab()
    await waitFor(() => {
      expect(api.reportPnl).toHaveBeenCalledTimes(2)
    })

    await act(async () => {
      second.resolve(
        pnl({
          from: '2025-01-01',
          to: monthEndISO(),
          expenses: [expense({ code: '5100', name: 'Groceries', balance_minor: 40_00 })],
          total_expenses: 40_00,
          net_income: -40_00,
        }),
      )
    })
    await waitFor(() => {
      expect(screen.getAllByText('Groceries').length).toBeGreaterThan(0)
    })

    await act(async () => {
      first.reject({
        code: 'date_range_inverted',
        message: 'from date must be on or before to',
      })
    })
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getAllByText('Groceries').length).toBeGreaterThan(0)
  })

  test('changing As of on the balance sheet refetches that date', async () => {
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await user.click(screen.getByRole('button', { name: 'Balance sheet' }))
    await waitFor(() => {
      expect(api.reportBalanceSheet).toHaveBeenCalledWith('e1', todayISO())
    })

    const asOf = screen.getByLabelText('Report as-of date')
    await user.clear(asOf)
    await user.type(asOf, '31/12/2025')
    await user.tab()

    await waitFor(() => {
      expect(api.reportBalanceSheet).toHaveBeenCalledWith('e1', '2025-12-31')
    })
  })

  test('changing To refetches P&L for the new window', async () => {
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(api.reportPnl).toHaveBeenCalledWith('e1', monthStartISO(), monthEndISO())
    })

    const to = screen.getByLabelText('Report to date')
    await user.clear(to)
    await user.type(to, '30/06/2025')
    await user.tab()

    await waitFor(() => {
      expect(api.reportPnl).toHaveBeenCalledWith('e1', monthStartISO(), '2025-06-30')
    })
  })

  test('changing As of on the trial balance refetches that date', async () => {
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await user.click(screen.getByRole('button', { name: 'Trial balance' }))
    await waitFor(() => {
      expect(api.reportTrialBalance).toHaveBeenCalledWith('e1', todayISO())
    })

    const asOf = screen.getByLabelText('Report as-of date')
    await user.clear(asOf)
    await user.type(asOf, '15/03/2025')
    await user.tab()

    await waitFor(() => {
      expect(api.reportTrialBalance).toHaveBeenCalledWith('e1', '2025-03-15')
    })
  })

  test('Refresh re-runs the current report', async () => {
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(api.reportPnl).toHaveBeenCalledTimes(1)
    })
    await user.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => {
      expect(api.reportPnl).toHaveBeenCalledTimes(2)
    })
  })
})

describe('ReportsPage statements', () => {
  test('shows the fetch error instead of a stale statement', async () => {
    vi.mocked(api.reportPnl).mockRejectedValue({
      code: 'date_range_inverted',
      message: 'from date must be on or before to',
    })
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByRole('alert').textContent).toBe('The From date must be on or before the To date.')
    })
    expect(screen.queryByText('Profit & Loss')).toBeNull()
  })

  test('a fetch failure with an unknown code never shows the raw message', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.reportPnl).mockRejectedValue({
      code: 'brand_new',
      message: 'sqlcipher: disk image is malformed',
    })
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByRole('alert').textContent).toBe('Something went wrong.')
    })
  })

  test('empty P&L period shows the empty-line copy', async () => {
    vi.mocked(api.reportPnl).mockResolvedValue(
      pnl({
        from: monthStartISO(),
        to: monthEndISO(),
        income: [],
        expenses: [],
        total_income: 0,
        total_expenses: 0,
        net_income: 0,
      }),
    )
    render(<ReportsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByText('No income recorded in this period.')).toBeTruthy()
    })
    expect(screen.getByText('No expenses recorded in this period.')).toBeTruthy()
  })

  test('balanced books show the balance copy', async () => {
    const sheet: BalanceSheet = {
      entity_id: 'e1',
      as_of: todayISO(),
      assets: {
        title: 'Assets',
        lines: [expense({ code: '1010', name: 'Checking', balance_minor: 100_00 })],
        total: 100_00,
      },
      liabilities: { title: 'Liabilities', lines: [], total: 0 },
      equity: {
        title: 'Equity',
        lines: [expense({ code: 'NI', name: 'Net Income (current period)', balance_minor: 100_00 })],
        total: 100_00,
      },
      total_assets: 100_00,
      total_liabilities_equity: 100_00,
    }
    vi.mocked(api.reportBalanceSheet).mockResolvedValue(sheet)
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await user.click(screen.getByRole('button', { name: 'Balance sheet' }))
    await waitFor(() => {
      expect(
        screen.getByText('Assets equal liabilities plus equity — the books balance.'),
      ).toBeTruthy()
    })
  })

  test('out-of-balance sheet shows the difference', async () => {
    const sheet: BalanceSheet = {
      entity_id: 'e1',
      as_of: todayISO(),
      assets: { title: 'Assets', lines: [], total: 50_00 },
      liabilities: { title: 'Liabilities', lines: [], total: 0 },
      equity: { title: 'Equity', lines: [], total: 20_00 },
      total_assets: 50_00,
      total_liabilities_equity: 20_00,
    }
    vi.mocked(api.reportBalanceSheet).mockResolvedValue(sheet)
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await user.click(screen.getByRole('button', { name: 'Balance sheet' }))
    await waitFor(() => {
      expect(screen.getByText(/Out of balance by/)).toBeTruthy()
    })
  })

  test('trial balance paints debit and credit columns', async () => {
    const tb: TrialBalance = {
      entity_id: 'e1',
      as_of: todayISO(),
      lines: [
        {
          code: '1010',
          name: 'Checking',
          account_type: 'asset',
          debit_minor: 0,
          credit_minor: 1_000,
          balance_minor: -1_000,
        },
        {
          code: '5100',
          name: 'Food',
          account_type: 'expense',
          debit_minor: 1_000,
          credit_minor: 0,
          balance_minor: 1_000,
        },
      ],
      total_debits: 1_000,
      total_credits: 1_000,
    }
    vi.mocked(api.reportTrialBalance).mockResolvedValue(tb)
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await user.click(screen.getByRole('button', { name: 'Trial balance' }))
    await waitFor(() => {
      expect(screen.getByText('Checking')).toBeTruthy()
    })
    expect(screen.getByText('Food')).toBeTruthy()
    expect(screen.getByText('Debit')).toBeTruthy()
    expect(screen.getByText('Credit')).toBeTruthy()
  })
})

describe('ReportsPage empty-state CTA', () => {
  test('no-book empty state renders Create a book and calls onCreateBook', async () => {
    const onCreateBook = vi.fn()
    render(<ReportsPage entity={null} onCreateBook={onCreateBook} />)
    const cta = screen.getByRole('button', { name: 'Create a book' })
    await userEvent.click(cta)
    expect(onCreateBook).toHaveBeenCalledTimes(1)
  })

  test('renders without a CTA when onCreateBook is not supplied', () => {
    render(<ReportsPage entity={null} />)
    expect(screen.queryByRole('button', { name: 'Create a book' })).toBeNull()
  })
})

describe('ReportsPage synthetic rows', () => {
  const retained = expense({
    code: 'RE',
    name: 'Retained Earnings (prior periods)',
    account_type: 'equity',
    balance_minor: 40_00,
    synthetic: 'retained_earnings',
  })
  const netIncome = expense({
    code: 'NI',
    name: 'Net Income (current period)',
    account_type: 'equity',
    balance_minor: 60_00,
    synthetic: 'net_income',
  })

  test('the balance sheet words the computed rows in the current language', async () => {
    vi.mocked(api.reportBalanceSheet).mockResolvedValue({
      entity_id: 'e1',
      as_of: todayISO(),
      assets: { title: 'Assets', lines: [], total: 100_00 },
      liabilities: { title: 'Liabilities', lines: [], total: 0 },
      equity: { title: 'Equity', lines: [retained, netIncome], total: 100_00 },
      total_assets: 100_00,
      total_liabilities_equity: 100_00,
    })
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await user.click(screen.getByRole('button', { name: 'Balance sheet' }))
    await waitFor(() => {
      expect(screen.getByText('Net Income (current period)')).toBeTruthy()
    })

    act(() => setLocale('el'))

    expect(screen.getByText('Παρακρατηθέντα κέρδη (προηγούμενες περίοδοι)')).toBeTruthy()
    expect(screen.getByText('Καθαρό αποτέλεσμα (τρέχουσα περίοδος)')).toBeTruthy()
    expect(screen.queryByText('Net Income (current period)')).toBeNull()

    act(() => setLocale('de'))

    expect(screen.getByText('Gewinnvortrag (frühere Perioden)')).toBeTruthy()
    expect(screen.getByText('Periodenergebnis (aktuelle Periode)')).toBeTruthy()
  })

  test('the trial balance words the computed rows too', async () => {
    vi.mocked(api.reportTrialBalance).mockResolvedValue({
      entity_id: 'e1',
      as_of: todayISO(),
      lines: [retained],
      total_debits: 0,
      total_credits: 40_00,
    })
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await user.click(screen.getByRole('button', { name: 'Trial balance' }))
    await waitFor(() => {
      expect(screen.getByText('Retained Earnings (prior periods)')).toBeTruthy()
    })

    act(() => setLocale('fr'))

    expect(screen.getByText('Report à nouveau (périodes précédentes)')).toBeTruthy()
  })

  test('a real account keeps its own name in every language', async () => {
    vi.mocked(api.reportTrialBalance).mockResolvedValue({
      entity_id: 'e1',
      as_of: todayISO(),
      lines: [expense({ code: '1010', name: 'Checking', account_type: 'asset', balance_minor: 10_00 })],
      total_debits: 10_00,
      total_credits: 0,
    })
    const user = userEvent.setup()
    render(<ReportsPage entity={entity} />)
    await user.click(screen.getByRole('button', { name: 'Trial balance' }))
    await waitFor(() => {
      expect(screen.getByText('Checking')).toBeTruthy()
    })

    act(() => setLocale('el'))

    expect(screen.getByText('Checking')).toBeTruthy()
  })
})
