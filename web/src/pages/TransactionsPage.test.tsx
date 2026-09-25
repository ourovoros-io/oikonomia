/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type {
  Account,
  CsvImportPreview,
  DocumentSuggestion,
  Entity,
  PendingDocSource,
  PostedEntryView,
  SimpleEntryInput,
} from '../lib/api'

const dropZone = vi.hoisted(() => ({
  onSuggestion: null as
    | ((suggestion: DocumentSuggestion, source: PendingDocSource) => void)
    | null,
}))

vi.mock('../components/DocumentDropZone', () => ({
  DocumentDropZone: ({
    onSuggestion,
  }: {
    onSuggestion: (suggestion: DocumentSuggestion, source: PendingDocSource) => void
  }) => {
    dropZone.onSuggestion = onSuggestion
    return <div data-testid="document-drop-zone" />
  },
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
      entrySetHidden: vi.fn(),
      recurringList: vi.fn(),
      recurringCreate: vi.fn(),
      recurringPost: vi.fn(),
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

const incomeEntry: PostedEntryView = {
  entry: {
    id: 'j2',
    entity_id: 'e1',
    entry_date: '2026-08-11',
    description: 'CLIENT INVOICE',
    reference: null,
    status: 'posted',
    hidden: false,
  },
  lines: [
    {
      id: 'l3',
      entry_id: 'j2',
      account_id: 'w1',
      debit: { amount_minor: 120000 },
      credit: { amount_minor: 0 },
      memo: null,
    },
    {
      id: 'l4',
      entry_id: 'j2',
      account_id: 'inc1',
      debit: { amount_minor: 0 },
      credit: { amount_minor: 120000 },
      memo: null,
    },
  ],
  is_voided: false,
}

function documentSuggestion(over: Partial<DocumentSuggestion> = {}): DocumentSuggestion {
  return {
    source: 'heuristic',
    model: null,
    kind: 'expense',
    amount_minor: null,
    entry_date: null,
    description: null,
    reference: null,
    merchant: null,
    bill_unpaid: false,
    category_account_id: null,
    wallet_account_id: null,
    payable_account_id: null,
    confidence: 0.4,
    notes: '',
    ...over,
  }
}

afterEach(() => {
  cleanup()
  dropZone.onSuggestion = null
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
  vi.mocked(api.entrySetHidden).mockReset().mockImplementation(async (id, hidden) => ({
    ...postedEntry,
    entry: { ...postedEntry.entry, id, hidden },
  }))
  vi.mocked(api.recurringList).mockReset().mockResolvedValue([])
  vi.mocked(api.recurringCreate).mockReset()
  vi.mocked(api.recurringPost).mockReset()
})

async function renderReady() {
  render(<TransactionsPage entity={entity} />)
  await waitFor(() => {
    expect(screen.getByRole('button', { name: 'Import CSV' })).toBeTruthy()
  })
}

describe('TransactionsPage entry kind colours', () => {
  test('income and expense rows wear the Ledger money tones, never status colours', async () => {
    vi.mocked(api.entryList).mockReset().mockResolvedValue([postedEntry, incomeEntry])
    await renderReady()
    await waitFor(() => {
      expect(screen.getByText('CLIENT INVOICE')).toBeTruthy()
    })

    const expenseBadge = screen.getByText('Alpha supermarket').closest('li')?.firstElementChild
    expect(expenseBadge?.className).toContain('bg-[var(--color-money-out-soft)]')

    const incomeBadge = screen.getByText('CLIENT INVOICE').closest('li')?.firstElementChild
    expect(incomeBadge?.className).toContain('bg-[var(--color-money-in-soft)]')
  })
})

describe('TransactionsPage CSV toolbar', () => {
  test('toolbar shows Recurring left of Import CSV, Export CSV, New Entry', async () => {
    await renderReady()
    const names = screen.getAllByRole('button').map((el) => el.textContent?.replace(/\s+/g, ' ').trim())
    const recurring = names.indexOf('Recurring')
    const importCsv = names.indexOf('Import CSV')
    const exportCsv = names.indexOf('Export CSV')
    const newEntry = names.indexOf('New Entry')
    expect(recurring).toBeGreaterThanOrEqual(0)
    expect(importCsv).toBeGreaterThan(recurring)
    expect(exportCsv).toBeGreaterThan(importCsv)
    expect(newEntry).toBeGreaterThan(exportCsv)
  })

  test('Recurring opens the empty templates sub-view', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Recurring' }))
    await waitFor(() => {
      expect(screen.getByText('No recurring templates yet')).toBeTruthy()
    })
    expect(api.recurringList).toHaveBeenCalledWith('e1')
    expect(screen.queryByRole('button', { name: 'Import CSV' })).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Back to entries' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Import CSV' })).toBeTruthy()
    })
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
    expect(screen.queryByRole('checkbox', { name: /include hidden/i })).toBeNull()
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

  test('invalid amount marks the Amount field invalid and describes the error', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'New Entry' }))
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New entry' })).toBeTruthy()
    })
    const amountField = screen.getByRole('textbox', { name: /amount/i })
    expect(amountField).not.toHaveAttribute('aria-invalid')
    await userEvent.type(screen.getByLabelText(/description/i), 'Coffee')
    await userEvent.type(amountField, '0')
    await userEvent.click(screen.getByRole('button', { name: 'Save entry' }))
    await waitFor(() => {
      expect(amountField).toHaveAttribute('aria-invalid', 'true')
    })
    expect(amountField).toHaveAccessibleDescription(
      'Enter a valid amount (e.g. 25.50 or 25,50)',
    )

    await userEvent.type(amountField, '12.50')
    expect(amountField).not.toHaveAttribute('aria-invalid')
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

describe('TransactionsPage hidden paint', () => {
  const hiddenEntry: PostedEntryView = {
    ...postedEntry,
    entry: { ...postedEntry.entry, id: 'j2', description: 'ATM cash', hidden: true },
  }

  test('list shows Hidden pill, posted/hidden counts, and export whisper', async () => {
    vi.mocked(api.entryList).mockResolvedValue([postedEntry, hiddenEntry])
    await renderReady()
    expect(screen.getByText('1 posted · 1 hidden · EUR')).toBeTruthy()
    expect(screen.getByText('Export omits hidden rows.')).toBeTruthy()
    expect(screen.getByText('Hidden')).toBeTruthy()
    expect(screen.getByText('ATM cash')).toBeTruthy()
    expect(screen.queryByRole('checkbox', { name: /include hidden/i })).toBeNull()
    expect(screen.queryByText('Hide from export')).toBeNull()
  })

  test('EL list badge is Κρυφή', async () => {
    const { setLocale } = await import('../lib/i18n')
    setLocale('el')
    vi.mocked(api.entryList).mockResolvedValue([hiddenEntry])
    render(<TransactionsPage entity={entity} />)
    await waitFor(() => {
      expect(screen.getByText('Κρυφή')).toBeTruthy()
    })
    expect(screen.getByText('0 καταχωρισμένα · 1 κρυφά · EUR')).toBeTruthy()
    expect(screen.getByText('Η εξαγωγή παραλείπει τις κρυφές γραμμές.')).toBeTruthy()
  })

  test('detail Hide checkbox calls entry_set_hidden', async () => {
    await renderReady()
    await userEvent.click(screen.getByText('Alpha supermarket'))
    await waitFor(() => {
      expect(screen.getByRole('checkbox', { name: /hide/i })).toBeTruthy()
    })
    expect(screen.getByText('Hidden from export')).toBeTruthy()
    expect(screen.queryByRole('radio', { name: /hide/i })).toBeNull()
    await userEvent.click(screen.getByRole('checkbox', { name: /hide/i }))
    await waitFor(() => {
      expect(api.entrySetHidden).toHaveBeenCalledWith('j1', true)
    })
  })

  test('unhide calls entrySetHidden(id, false)', async () => {
    vi.mocked(api.entryList).mockResolvedValue([hiddenEntry])
    await renderReady()
    await userEvent.click(screen.getByText('ATM cash'))
    await waitFor(() => {
      expect(screen.getByRole('checkbox', { name: /hide/i })).toBeChecked()
    })
    await userEvent.click(screen.getByRole('checkbox', { name: /hide/i }))
    await waitFor(() => {
      expect(api.entrySetHidden).toHaveBeenCalledWith('j2', false)
    })
  })

  test('setHidden reject shows ErrorBanner', async () => {
    vi.mocked(api.entrySetHidden).mockRejectedValue({
      code: 'unknown',
      message: 'could not hide entry',
    })
    await renderReady()
    await userEvent.click(screen.getByText('Alpha supermarket'))
    await waitFor(() => {
      expect(screen.getByRole('checkbox', { name: /hide/i })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('checkbox', { name: /hide/i }))
    await waitFor(() => {
      expect(screen.getByText('could not hide entry')).toBeTruthy()
    })
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

async function fillLeftoverDraft() {
  await userEvent.click(screen.getByRole('button', { name: 'New Entry' }))
  await waitFor(() => {
    expect(screen.getByRole('heading', { name: 'New entry' })).toBeTruthy()
  })
  await userEvent.type(screen.getByLabelText('Description'), 'LEFTOVER DRAFT PAYEE')
  await userEvent.type(screen.getByLabelText('Amount (EUR)'), '99.99')
  await userEvent.type(screen.getByLabelText('Reference (optional)'), 'DRAFT-REF-999')
}

function deliverDrop(suggestion: DocumentSuggestion) {
  const deliver = dropZone.onSuggestion
  expect(deliver).toBeTruthy()
  act(() => {
    deliver!(suggestion, { kind: 'path', path: '/tmp/receipt.pdf' })
  })
}

describe('TransactionsPage discards unfinished draft', () => {
  test('close then drop uses that file suggestion, not leftover draft fields', async () => {
    await renderReady()
    await fillLeftoverDraft()
    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'New entry' })).toBeNull()
    })

    deliverDrop(
      documentSuggestion({
        amount_minor: 1234,
        description: null,
        reference: null,
        notes: 'Partial scan — amount only',
      }),
    )

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New entry' })).toBeTruthy()
    })
    expect(screen.getByLabelText('Description')).toHaveValue('')
    expect(screen.getByLabelText('Amount (EUR)')).toHaveValue('12.34')
    expect(screen.getByLabelText('Reference (optional)')).toHaveValue('')
    expect(screen.getByText('Partial scan — amount only')).toBeTruthy()
    expect(screen.queryByDisplayValue('LEFTOVER DRAFT PAYEE')).toBeNull()
    expect(screen.queryByDisplayValue('99.99')).toBeNull()
    expect(screen.queryByDisplayValue('DRAFT-REF-999')).toBeNull()
  })

  test('close then New Entry is a clean form, not the leftover draft', async () => {
    await renderReady()
    await fillLeftoverDraft()
    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'New entry' })).toBeNull()
    })

    await userEvent.click(screen.getByRole('button', { name: 'New Entry' }))
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New entry' })).toBeTruthy()
    })
    expect(screen.getByLabelText('Description')).toHaveValue('')
    expect(screen.getByLabelText('Amount (EUR)')).toHaveValue('')
    expect(screen.getByLabelText('Reference (optional)')).toHaveValue('')
    expect(screen.queryByDisplayValue('LEFTOVER DRAFT PAYEE')).toBeNull()
    expect(screen.queryByText('Partial scan — amount only')).toBeNull()
  })

  test('startEdit still populates from the posted entry after a discarded draft', async () => {
    await renderReady()
    await fillLeftoverDraft()
    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: 'New entry' })).toBeNull()
    })

    await userEvent.click(screen.getByText('Alpha supermarket'))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Edit entry' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Edit entry' }))
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Edit entry' })).toBeTruthy()
    })
    expect(screen.getByLabelText('Description')).toHaveValue('Alpha supermarket')
    expect(screen.getByLabelText('Amount (EUR)')).toHaveValue('42.50')
    expect(screen.getByLabelText('Reference (optional)')).toHaveValue('')
    expect(screen.queryByDisplayValue('LEFTOVER DRAFT PAYEE')).toBeNull()
    expect(screen.queryByDisplayValue('99.99')).toBeNull()
  })
})

describe('TransactionsPage empty-state CTA', () => {
  test('no-book empty state renders Create a book and calls onCreateBook', async () => {
    const onCreateBook = vi.fn()
    render(<TransactionsPage entity={null} onCreateBook={onCreateBook} />)
    const cta = screen.getByRole('button', { name: 'Create a book' })
    await userEvent.click(cta)
    expect(onCreateBook).toHaveBeenCalledTimes(1)
  })

  test('renders without a CTA when onCreateBook is not supplied', () => {
    render(<TransactionsPage entity={null} />)
    expect(screen.queryByRole('button', { name: 'Create a book' })).toBeNull()
  })
})
