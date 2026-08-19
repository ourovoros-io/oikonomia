/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { Account, CsvImportPreview, Entity, PostedEntryView, SimpleEntryInput } from '../lib/api'

vi.mock('../components/DocumentDropZone', () => ({
  DocumentDropZone: () => null,
}))

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      entryList: vi.fn(),
      accountList: vi.fn(),
      documentList: vi.fn(),
      getUiPrefs: vi.fn(),
      csvImportPreview: vi.fn(),
      csvImportPost: vi.fn(),
      csvExportJournal: vi.fn(),
    },
  }
})

import { api } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { TransactionsPage } from './TransactionsPage'

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

function account(over: Partial<Account> & Pick<Account, 'id' | 'name' | 'account_type'>): Account {
  return {
    entity_id: 'e1',
    code: '1000',
    parent_id: null,
    is_active: true,
    is_system: false,
    sort_order: 0,
    ...over,
  }
}

const accounts: Account[] = [
  account({ id: 'w1', name: 'Checking', account_type: 'asset', code: '1000' }),
  account({ id: 'exp1', name: 'Meals & dining', account_type: 'expense', code: '5000' }),
  account({ id: 'inc1', name: 'Salary', account_type: 'income', code: '4000' }),
]

const grocery: SimpleEntryInput = {
  entity_id: 'e1',
  kind: 'expense',
  bill_status: null,
  entry_date: '2026-08-12',
  description: 'ALPHA SUPERMARKET',
  reference: null,
  amount_minor: 4250,
  category_account_id: 'exp1',
  wallet_account_id: 'w1',
  payable_account_id: null,
  from_account_id: null,
  to_account_id: null,
}

const salary: SimpleEntryInput = {
  ...grocery,
  kind: 'income',
  description: 'CLIENT INVOICE',
  amount_minor: 120000,
  category_account_id: 'inc1',
  entry_date: '2026-08-11',
}

const preview: CsvImportPreview = {
  source: '/tmp/bank.csv',
  headers: ['Date', 'Payee', 'Notes', 'Amount'],
  detected_mapping: {
    date: 'Date',
    description: 'Payee',
    amount: 'Amount',
    debit: null,
    credit: null,
    reference: null,
  },
  rows: [
    {
      source_row: 2,
      duplicate: false,
      error: null,
      suggested: grocery,
      signed_amount_minor: -4250,
    },
    {
      source_row: 3,
      duplicate: true,
      error: null,
      suggested: salary,
      signed_amount_minor: 120000,
    },
    {
      source_row: 4,
      duplicate: false,
      error: 'invalid amount',
      suggested: null,
      signed_amount_minor: null,
    },
  ],
}

const postedEntry: PostedEntryView = {
  entry: {
    id: 'j1',
    entity_id: 'e1',
    entry_date: '2026-08-12',
    description: 'Alpha supermarket',
    reference: null,
    status: 'posted',
    hidden: false,
  },
  lines: [
    {
      id: 'l1',
      entry_id: 'j1',
      account_id: 'exp1',
      debit: { amount_minor: 4250 },
      credit: { amount_minor: 0 },
      memo: null,
    },
    {
      id: 'l2',
      entry_id: 'j1',
      account_id: 'w1',
      debit: { amount_minor: 0 },
      credit: { amount_minor: 4250 },
      memo: null,
    },
  ],
  is_voided: false,
}

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(api.entryList).mockReset().mockResolvedValue([postedEntry])
  vi.mocked(api.accountList).mockReset().mockResolvedValue(accounts)
  vi.mocked(api.documentList).mockReset().mockResolvedValue([])
  vi.mocked(api.getUiPrefs).mockReset().mockRejectedValue(new Error('no prefs'))
  vi.mocked(api.csvImportPreview).mockReset().mockResolvedValue(preview)
  vi.mocked(api.csvImportPost).mockReset().mockResolvedValue({
    posted: [postedEntry],
    skipped_duplicate_count: 0,
  })
  vi.mocked(api.csvExportJournal).mockReset().mockResolvedValue('/tmp/journal.csv')
})

async function renderReady() {
  render(<TransactionsPage entity={entity} />)
  await waitFor(() => {
    expect(screen.getByRole('button', { name: 'Import CSV' })).toBeTruthy()
  })
}

describe('TransactionsPage CSV toolbar', () => {
  test('toolbar shows Import CSV, Export CSV, New Entry', async () => {
    await renderReady()
    expect(screen.getByRole('button', { name: 'Import CSV' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Export CSV' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'New Entry' })).toBeTruthy()
  })

  test('Export CSV calls csvExportJournal with entityId', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Export CSV' }))
    await waitFor(() => {
      expect(api.csvExportJournal).toHaveBeenCalledWith('e1')
    })
    expect(api.csvExportJournal).toHaveBeenCalledTimes(1)
    expect(api.csvExportJournal.mock.calls[0]).toEqual(['e1'])
  })

  test('export uses the journal path with no include-hidden toggle', async () => {
    await renderReady()
    expect(screen.queryByRole('checkbox', { name: /include hidden/i })).toBeNull()
    expect(screen.queryByRole('switch', { name: /hidden/i })).toBeNull()
    expect(screen.queryByText('Hide from export')).toBeNull()
    expect(screen.queryByText('Show in export')).toBeNull()
    expect(screen.queryByText('Nothing to export. Every line is hidden.')).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Export CSV' }))
    await waitFor(() => {
      expect(api.csvExportJournal).toHaveBeenCalledWith('e1')
    })
  })

  test('all-hidden list still uses native journal export (no web empty handler)', async () => {
    vi.mocked(api.entryList).mockResolvedValue([
      {
        ...postedEntry,
        entry: { ...postedEntry.entry, hidden: true },
      },
    ])
    await renderReady()
    expect(screen.queryByText('Nothing to export. Every line is hidden.')).toBeNull()
    expect(screen.queryByText('Hidden')).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Export CSV' }))
    await waitFor(() => {
      expect(api.csvExportJournal).toHaveBeenCalledWith('e1')
    })
  })

  test('New Entry form has no Hidden checkbox (HOLD paint)', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'New Entry' }))
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New entry' })).toBeTruthy()
    })
    expect(screen.queryByRole('checkbox', { name: /hidden/i })).toBeNull()
    expect(screen.queryByText('Export skips this line. Backup still includes it.')).toBeNull()
    expect(screen.queryByText('Hide from export')).toBeNull()
  })

  test('cancelled import does not open mapping or preview', async () => {
    vi.mocked(api.csvImportPreview).mockResolvedValue(null)
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(api.csvImportPreview).toHaveBeenCalledTimes(1)
    })
    expect(screen.queryByRole('dialog', { name: 'Map CSV columns' })).toBeNull()
    expect(screen.queryByRole('dialog', { name: 'Preview import' })).toBeNull()
    expect(api.csvImportPost).not.toHaveBeenCalled()
  })
})

describe('TransactionsPage CSV mapping and preview', () => {
  test('import opens mapping; unchanged continue shows preview without a second preview call', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Map CSV columns' })).toBeTruthy()
    })
    expect(api.csvImportPreview).toHaveBeenCalledTimes(1)
    expect(api.csvImportPreview.mock.calls[0]?.[0]).not.toHaveProperty('path')
    expect(api.csvImportPreview.mock.calls[0]?.[0]).not.toHaveProperty('mapping')
    expect(api.csvImportPost).not.toHaveBeenCalled()

    await userEvent.click(screen.getByRole('button', { name: 'Continue to preview' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Preview import' })).toBeTruthy()
    })
    expect(api.csvImportPreview).toHaveBeenCalledTimes(1)
    expect(api.csvImportPost).not.toHaveBeenCalled()
  })

  test('mapping continue passes chosen headers and re-previews when they differ', async () => {
    const remapped: CsvImportPreview = {
      ...preview,
      rows: preview.rows.map((row) =>
        row.suggested
          ? { ...row, suggested: { ...row.suggested, description: 'ignored notes' } }
          : row,
      ),
    }
    vi.mocked(api.csvImportPreview)
      .mockResolvedValueOnce(preview)
      .mockResolvedValueOnce(remapped)

    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Description source column' })).toBeTruthy()
    })

    await userEvent.selectOptions(
      screen.getByRole('combobox', { name: 'Description source column' }),
      'Notes',
    )
    await userEvent.click(screen.getByRole('button', { name: 'Continue to preview' }))

    await waitFor(() => {
      expect(api.csvImportPreview).toHaveBeenCalledTimes(2)
    })
    expect(api.csvImportPreview).toHaveBeenNthCalledWith(2, {
      entity_id: 'e1',
      path: '/tmp/bank.csv',
      wallet_account_id: 'w1',
      expense_account_id: 'exp1',
      income_account_id: 'inc1',
      mapping: {
        date: 'Date',
        description: 'Notes',
        amount: 'Amount',
        debit: null,
        credit: null,
        reference: null,
      },
    })
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Preview import' })).toBeTruthy()
    })
    expect(api.csvImportPost).not.toHaveBeenCalled()
  })

  test('without headers, mapping shows Auto-detected and continue uses the existing preview', async () => {
    const legacy: CsvImportPreview = {
      source: '/tmp/bank.csv',
      rows: preview.rows,
    }
    vi.mocked(api.csvImportPreview).mockResolvedValue(legacy)

    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Map CSV columns' })).toBeTruthy()
    })
    expect(screen.getAllByDisplayValue('Auto-detected').length).toBeGreaterThan(0)
    expect(screen.queryByRole('combobox', { name: 'Description source column' })).toBeNull()

    await userEvent.click(screen.getByRole('button', { name: 'Continue to preview' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Preview import' })).toBeTruthy()
    })
    expect(api.csvImportPreview).toHaveBeenCalledTimes(1)
    expect(api.csvImportPreview.mock.calls[0]?.[0]).not.toHaveProperty('mapping')
    expect(api.csvImportPost).not.toHaveBeenCalled()
  })

  test('duplicate rows are unchecked; Post N selected matches checked count', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Continue to preview' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Continue to preview' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Preview import' })).toBeTruthy()
    })

    expect(screen.getByRole('checkbox', { name: 'Select row 2' })).toBeChecked()
    expect(screen.getByRole('checkbox', { name: 'Select row 3' })).not.toBeChecked()
    expect(screen.getByRole('checkbox', { name: 'Select row 4' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Post 1 selected' })).toBeEnabled()

    await userEvent.click(screen.getByRole('checkbox', { name: 'Select row 3' }))
    expect(screen.getByRole('button', { name: 'Post 2 selected' })).toBeEnabled()
  })

  test('Post calls csvImportPost with selected rows only; import does not auto-post', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Continue to preview' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Continue to preview' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Post 1 selected' })).toBeTruthy()
    })
    expect(api.csvImportPost).not.toHaveBeenCalled()

    await userEvent.click(screen.getByRole('button', { name: 'Post 1 selected' }))
    await waitFor(() => {
      expect(api.csvImportPost).toHaveBeenCalledTimes(1)
    })
    expect(api.csvImportPost).toHaveBeenCalledWith({
      rows: [grocery],
      include_duplicates: false,
    })
  })
})
