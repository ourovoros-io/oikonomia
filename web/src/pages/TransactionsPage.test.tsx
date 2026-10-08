/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type {
  Account,
  AccountDefaults,
  CashFlowSeries,
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

// jsdom has no 2D canvas; the light's painting is tested on its own.
vi.mock('../components/CashFlowPulse', () => ({
  CashFlowPulse: ({ label }: { label: string }) => <div role="img" aria-label={label} />,
}))

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      entryList: vi.fn(),
      accountList: vi.fn(),
      accountDefaults: vi.fn(),
      documentList: vi.fn(),
      getUiPrefs: vi.fn(),
      csvImportPreview: vi.fn(),
      csvImportPost: vi.fn(),
      csvExportJournal: vi.fn(),
      entrySetHidden: vi.fn(),
      entryPostSimple: vi.fn(),
      recurringList: vi.fn(),
      recurringCreate: vi.fn(),
      recurringPost: vi.fn(),
      cashFlowSeries: vi.fn(),
    },
  }
})

import { api } from '../lib/api'
import { commandErrorMessage } from '../lib/commandError'
import { resetI18nForTests } from '../lib/i18n'
import { formatMoney } from '../lib/money'
import { TransactionsPage } from './TransactionsPage'

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  base_currency_decimals: 2,
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

const series: CashFlowSeries = {
  entity_id: 'e1',
  from: '2026-08-01',
  to: '2026-08-31',
  granularity: 'day',
  total_income_minor: 120000,
  total_expenses_minor: 4250,
  net_minor: 115750,
  buckets: [],
}

/** Rendered money carries a no-break space; text matchers see a plain one. */
const plain = (text: string) => text.replace(/\s/g, ' ')

function account(over: Partial<Account> & Pick<Account, 'id' | 'name' | 'account_type'>): Account {
  return {
    entity_id: 'e1',
    code: '1000',
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
      error: { code: 'csv_invalid_amount', params: { value: 'abc' } },
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
    notes: [],
    ...over,
  }
}

const DEFAULTS: AccountDefaults = {
  category: 'exp1',
  payment: 'w1',
  deposit: 'w1',
  income: 'inc1',
  bill_category: 'exp1',
  bills_payable: null,
  transfer_source: 'w1',
  transfer_destination: 'w1',
}

afterEach(() => {
  cleanup()
  dropZone.onSuggestion = null
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(api.entryList).mockReset().mockResolvedValue([postedEntry])
  vi.mocked(api.accountList).mockReset().mockResolvedValue(accounts)
  vi.mocked(api.accountDefaults).mockReset().mockResolvedValue(DEFAULTS)
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
  vi.mocked(api.cashFlowSeries).mockReset().mockResolvedValue(series)
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
  test('the list header carries Recurring, Import CSV and Export CSV; New Entry sits in the top bar', async () => {
    await renderReady()
    const names = screen.getAllByRole('button').map((el) => el.textContent?.replace(/\s+/g, ' ').trim())
    const newEntry = names.indexOf('New Entry')
    const recurring = names.indexOf('Recurring')
    const importCsv = names.indexOf('Import CSV')
    const exportCsv = names.indexOf('Export CSV')
    expect(newEntry).toBeGreaterThanOrEqual(0)
    expect(recurring).toBeGreaterThan(newEntry)
    expect(importCsv).toBeGreaterThan(recurring)
    expect(exportCsv).toBeGreaterThan(importCsv)
    expect(names.filter((name) => name === 'New Entry')).toHaveLength(1)
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
    // Zero is a number, so it gets its own sentence, not the typo one.
    expect(amountField).toHaveAccessibleDescription('The amount must be greater than zero.')

    await userEvent.type(amountField, '12.50')
    expect(amountField).not.toHaveAttribute('aria-invalid')
  })

  test('a failed save is explained inside the dialog and gone after Cancel', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'New Entry' }))
    const dialog = await screen.findByRole('dialog', { name: 'New entry' })
    await userEvent.type(within(dialog).getByLabelText(/description/i), 'Coffee')
    await userEvent.type(within(dialog).getByRole('textbox', { name: /amount/i }), 'abc')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Save entry' }))

    expect(within(dialog).getByRole('alert')).toHaveTextContent('Enter a valid amount')

    await userEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    await userEvent.click(screen.getByRole('button', { name: 'New Entry' }))
    const reopened = await screen.findByRole('dialog', { name: 'New entry' })

    expect(screen.queryByRole('alert')).toBeNull()
    expect(within(reopened).queryByRole('alert')).toBeNull()
  })

  test('Enter in the amount field saves the entry, with native validation off', async () => {
    vi.mocked(api.entryPostSimple).mockResolvedValue(undefined as never)
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'New Entry' }))
    const dialog = await screen.findByRole('dialog', { name: 'New entry' })
    await userEvent.type(within(dialog).getByLabelText(/description/i), 'Coffee')
    await userEvent.type(within(dialog).getByRole('textbox', { name: /amount/i }), '4,20{Enter}')

    await waitFor(() => {
      expect(api.entryPostSimple).toHaveBeenCalledTimes(1)
    })
    expect(vi.mocked(api.entryPostSimple).mock.calls[0]?.[0]).toMatchObject({ amount_minor: 420 })
  })

  test('an empty description is reported in the app own words, not a browser bubble', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'New Entry' }))
    const dialog = await screen.findByRole('dialog', { name: 'New entry' })
    await userEvent.type(within(dialog).getByRole('textbox', { name: /amount/i }), '4,20{Enter}')

    expect(within(dialog).getByRole('alert')).toHaveTextContent('A required field is empty')
    expect(api.entryPostSimple).not.toHaveBeenCalled()
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
      message: 'sqlite: could not hide entry',
    })
    await renderReady()
    await userEvent.click(screen.getByText('Alpha supermarket'))
    await waitFor(() => {
      expect(screen.getByRole('checkbox', { name: /hide/i })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('checkbox', { name: /hide/i }))
    await waitFor(() => {
      expect(screen.getByText('Could not update the entry.')).toBeTruthy()
    })
    expect(screen.queryByText(/sqlite/)).toBeNull()
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
        direction: null,
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

describe('TransactionsPage CSV import of a file whose columns were not detected', () => {
  /** What Rust returns for `When,Memo,Paid`: the headers, no date, no amount, no rows. */
  const undetected: CsvImportPreview = {
    source: '/tmp/bank.csv',
    headers: ['When', 'Memo', 'Paid'],
    detected_mapping: {
      date: null,
      description: 'Memo',
      amount: null,
      debit: null,
      credit: null,
      reference: null,
      direction: null,
    },
    missing_columns: ['date', 'amount'],
    rows: [],
  }

  async function openMapping(first: CsvImportPreview) {
    vi.mocked(api.csvImportPreview).mockResolvedValueOnce(first)
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Map CSV columns' })).toBeTruthy()
    })
  }

  test('mapping opens with the reason, the detected column chosen and the missing ones empty', async () => {
    await openMapping(undetected)

    expect(screen.getByRole('status')).toHaveTextContent(
      'The date and amount columns could not be found automatically, so choose them below.',
    )
    expect(screen.getByRole('combobox', { name: 'Date source column' })).toHaveValue('')
    expect(screen.getByRole('combobox', { name: 'Amount source column' })).toHaveValue('')
    expect(screen.getByRole('combobox', { name: 'Description source column' })).toHaveValue('Memo')
    expect(screen.getByRole('button', { name: 'Continue to preview' })).toBeDisabled()
    expect(screen.queryByText(/No date column was found/)).toBeNull()
  })

  test.each([
    [['date'], 'The date column could not be found automatically, so choose it below.'],
    [['amount'], 'The amount column could not be found automatically, so choose it below.'],
  ] as const)('the reason names the one missing column: %j', async (missing, sentence) => {
    await openMapping({ ...undetected, missing_columns: [...missing] })

    expect(screen.getByRole('status')).toHaveTextContent(sentence)
  })

  test('a file that was detected in full gives no reason', async () => {
    await openMapping({ ...preview, missing_columns: [] })

    expect(screen.queryByRole('status')).toBeNull()
  })

  test('continuing with a complete mapping previews the file again with the chosen columns', async () => {
    vi.mocked(api.csvImportPreview)
      .mockResolvedValueOnce(undetected)
      .mockResolvedValueOnce({ ...preview, headers: undetected.headers ?? [] })
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Date source column' })).toBeTruthy()
    })

    await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Date source column' }), 'When')
    await userEvent.selectOptions(
      screen.getByRole('combobox', { name: 'Amount source column' }),
      'Paid',
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
        date: 'When',
        description: 'Memo',
        amount: 'Paid',
        debit: null,
        credit: null,
        reference: null,
        direction: null,
      },
    })
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Preview import' })).toBeTruthy()
    })
    expect(screen.getByRole('button', { name: 'Post 1 selected' })).toBeEnabled()
    expect(api.csvImportPost).not.toHaveBeenCalled()
  })

  test('cancelling returns to the list with nothing previewed again and nothing posted', async () => {
    await openMapping(undetected)

    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))

    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Map CSV columns' })).toBeNull()
    })
    expect(screen.queryByRole('dialog', { name: 'Preview import' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Import CSV' })).toBeEnabled()
    expect(api.csvImportPreview).toHaveBeenCalledTimes(1)
    expect(api.csvImportPost).not.toHaveBeenCalled()
  })
})

describe('TransactionsPage CSV import of a file with one debit column', () => {
  /** What Rust detects for `Date,Memo,Notes,Debit`: money out only, no credit column. */
  const debitOnly: CsvImportPreview = {
    ...preview,
    headers: ['Date', 'Memo', 'Notes', 'Debit'],
    detected_mapping: {
      date: 'Date',
      description: 'Memo',
      amount: null,
      debit: 'Debit',
      credit: null,
      reference: null,
      direction: null,
    },
    missing_columns: [],
  }

  test('an edited mapping continues with the debit column alone', async () => {
    vi.mocked(api.csvImportPreview).mockResolvedValue(debitOnly)
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Debit source column' })).toHaveValue('Debit')
    })
    expect(screen.getByRole('combobox', { name: 'Credit source column' })).toHaveValue('')

    await userEvent.selectOptions(
      screen.getByRole('combobox', { name: 'Description source column' }),
      'Notes',
    )
    await userEvent.click(screen.getByRole('button', { name: 'Continue to preview' }))

    await waitFor(() => {
      expect(api.csvImportPreview).toHaveBeenCalledTimes(2)
    })
    expect(vi.mocked(api.csvImportPreview).mock.calls[1]?.[0].mapping).toEqual({
      date: 'Date',
      description: 'Notes',
      amount: null,
      debit: 'Debit',
      credit: null,
      reference: null,
      direction: null,
    })
  })

  test('a chosen credit column can be set back to not mapped', async () => {
    vi.mocked(api.csvImportPreview).mockResolvedValue(debitOnly)
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Credit source column' })).toBeTruthy()
    })
    const credit = screen.getByRole('combobox', { name: 'Credit source column' })

    await userEvent.selectOptions(credit, 'Notes')
    await userEvent.selectOptions(credit, 'Not mapped')

    expect(credit).toHaveValue('')
    expect(screen.getByRole('button', { name: 'Continue to preview' })).toBeEnabled()
  })
})

describe('TransactionsPage CSV import of a file without a description column', () => {
  /** What Rust returns for `Date,Amount`: read in full, with no description detected. */
  const noDescription: CsvImportPreview = {
    ...preview,
    headers: ['Date', 'Amount'],
    detected_mapping: {
      date: 'Date',
      description: null,
      amount: 'Amount',
      debit: null,
      credit: null,
      reference: null,
      direction: null,
    },
    missing_columns: [],
  }

  test('the description is optional, and Continue shows the rows Rust already read', async () => {
    vi.mocked(api.csvImportPreview).mockResolvedValue(noDescription)
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Description source column' })).toHaveValue('')
    })
    expect(screen.getByText('Description (optional)')).toBeTruthy()
    expect(screen.queryByRole('status')).toBeNull()

    await userEvent.click(screen.getByRole('button', { name: 'Continue to preview' }))

    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Preview import' })).toBeTruthy()
    })
    expect(api.csvImportPreview).toHaveBeenCalledTimes(1)
  })

  test('a detected description can be set to not mapped, which sends none', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Description source column' })).toHaveValue(
        'Payee',
      )
    })

    await userEvent.selectOptions(
      screen.getByRole('combobox', { name: 'Description source column' }),
      'Not mapped',
    )
    await userEvent.click(screen.getByRole('button', { name: 'Continue to preview' }))

    await waitFor(() => {
      expect(api.csvImportPreview).toHaveBeenCalledTimes(2)
    })
    expect(vi.mocked(api.csvImportPreview).mock.calls[1]?.[0].mapping).toEqual({
      date: 'Date',
      description: null,
      amount: 'Amount',
      debit: null,
      credit: null,
      reference: null,
      direction: null,
    })
  })
})

describe('TransactionsPage CSV direction column', () => {
  /** A statement with unsigned amounts and a column that says which way each went. */
  const withDirection: CsvImportPreview = {
    ...preview,
    headers: ['Date', 'Payee', 'Amount', 'Type', 'Way'],
    detected_mapping: { ...preview.detected_mapping, direction: 'Type' },
    missing_columns: [],
  }

  async function openMapping() {
    vi.mocked(api.csvImportPreview).mockResolvedValue(withDirection)
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'Import CSV' }))
    await waitFor(() => {
      expect(screen.getByRole('combobox', { name: 'Direction source column' })).toBeTruthy()
    })
  }

  test('the detected direction column is preselected and another one is sent when chosen', async () => {
    await openMapping()
    const direction = screen.getByRole('combobox', { name: 'Direction source column' })
    expect(direction).toHaveValue('Type')

    await userEvent.selectOptions(direction, 'Way')
    await userEvent.click(screen.getByRole('button', { name: 'Continue to preview' }))

    await waitFor(() => {
      expect(api.csvImportPreview).toHaveBeenCalledTimes(2)
    })
    expect(vi.mocked(api.csvImportPreview).mock.calls[1]?.[0].mapping).toEqual({
      date: 'Date',
      description: 'Payee',
      amount: 'Amount',
      debit: null,
      credit: null,
      reference: null,
      direction: 'Way',
    })
  })

  test('the direction can be set to not mapped, which sends none', async () => {
    await openMapping()

    await userEvent.selectOptions(
      screen.getByRole('combobox', { name: 'Direction source column' }),
      'Not mapped',
    )
    await userEvent.click(screen.getByRole('button', { name: 'Continue to preview' }))

    await waitFor(() => {
      expect(api.csvImportPreview).toHaveBeenCalledTimes(2)
    })
    expect(vi.mocked(api.csvImportPreview).mock.calls[1]?.[0].mapping?.direction).toBeNull()
  })

  test('debit and credit columns have no direction control', async () => {
    await openMapping()

    await userEvent.click(screen.getByRole('button', { name: 'Use debit and credit columns' }))

    expect(screen.queryByRole('combobox', { name: 'Direction source column' })).toBeNull()
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
        notes: [{ code: 'ocr_little_text' }],
      }),
    )

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'New entry' })).toBeTruthy()
    })
    expect(screen.getByLabelText('Description')).toHaveValue('')
    expect(screen.getByLabelText('Amount (EUR)')).toHaveValue('12.34')
    expect(screen.getByLabelText('Reference (optional)')).toHaveValue('')
    expect(
      screen.getByText('OCR ran but found little text — fill the form manually if needed.'),
    ).toBeTruthy()
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
    expect(screen.queryByText(/OCR ran but found little text/)).toBeNull()
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

describe('TransactionsPage default accounts', () => {
  test('New entry preselects the accounts Rust returned, whatever they are called', async () => {
    // Neither name holds an English word, and the default is not the first
    // expense account in the list: only Rust's answer can pick it.
    vi.mocked(api.accountList).mockResolvedValue([
      ...accounts,
      account({ id: 'exp2', name: 'Λογαριασμός 5100', account_type: 'expense', code: '5100' }),
    ])
    vi.mocked(api.accountDefaults).mockResolvedValue({ ...DEFAULTS, category: 'exp2' })

    render(<TransactionsPage entity={entity} newEntryIntent={1} onNewEntryIntentHandled={vi.fn()} />)
    await screen.findByRole('heading', { name: 'New entry' })

    await waitFor(() => {
      expect(screen.getByLabelText('Category (what for)')).toHaveValue('exp2')
    })
    expect(api.accountDefaults).toHaveBeenCalledWith('e1')
  })

  test('the loaded ledger and defaults are dropped when the book goes away', async () => {
    const { rerender } = render(
      <TransactionsPage entity={entity} newEntryIntent={0} onNewEntryIntentHandled={vi.fn()} />,
    )
    await waitFor(() => {
      expect(api.accountDefaults).toHaveBeenCalledTimes(1)
    })

    rerender(<TransactionsPage entity={null} newEntryIntent={0} onNewEntryIntentHandled={vi.fn()} />)

    // No book, so nothing is fetched again and nothing from the old one stays.
    expect(api.accountDefaults).toHaveBeenCalledTimes(1)
    expect(screen.queryByText('Meals & dining')).toBeNull()
  })
})

describe('TransactionsPage Quick add intent', () => {
  test('opens New entry once and reports the intent handled', async () => {
    const handled = vi.fn()
    render(<TransactionsPage entity={entity} newEntryIntent={1} onNewEntryIntentHandled={handled} />)

    expect(await screen.findByRole('heading', { name: 'New entry' })).toBeTruthy()
    expect(handled).toHaveBeenCalledTimes(1)
  })

  test('opens New entry from the Recurring sub-view too', async () => {
    const handled = vi.fn()
    const { rerender } = render(
      <TransactionsPage entity={entity} newEntryIntent={0} onNewEntryIntentHandled={handled} />,
    )
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Import CSV' })).toBeTruthy()
    })

    await userEvent.click(screen.getByRole('button', { name: 'Recurring' }))
    await waitFor(() => {
      expect(screen.getByText('No recurring templates yet')).toBeTruthy()
    })

    rerender(<TransactionsPage entity={entity} newEntryIntent={1} onNewEntryIntentHandled={handled} />)

    expect(await screen.findByRole('heading', { name: 'New entry' })).toBeTruthy()
    expect(handled).toHaveBeenCalledTimes(1)
  })
})

describe('TransactionsPage summary', () => {
  test('shows the Rust totals for the whole book when no dates are set', async () => {
    await renderReady()

    await waitFor(() => {
      expect(api.cashFlowSeries).toHaveBeenCalledWith('e1', null, null)
    })
    const heading = await screen.findByRole('heading', { name: 'In view · 01/08/2026 – 31/08/2026' })
    const pane = heading.closest('section')
    expect(pane?.querySelector('[data-net]')).toHaveAttribute('data-net', 'in')
    expect(pane?.querySelector('[data-net]')).toHaveTextContent(plain(formatMoney(115750, { code: 'EUR', decimals: 2 }, undefined, { signed: true })))
    expect(pane?.querySelector('[data-money-pill="in"]')).toHaveTextContent(plain(`In ${formatMoney(120000, { code: 'EUR', decimals: 2 })}`))
    expect(pane?.querySelector('[data-money-pill="out"]')).toHaveTextContent(plain(`Out ${formatMoney(4250, { code: 'EUR', decimals: 2 })}`))
  })

  test('a date filter narrows the summary', async () => {
    await renderReady()

    await userEvent.type(screen.getByLabelText('Filter from date'), '01/08/2026')
    await userEvent.tab()

    await waitFor(() => {
      expect(api.cashFlowSeries).toHaveBeenLastCalledWith('e1', '2026-08-01', null)
    })
  })

  test('the account filter leaves the summary whole, and says so', async () => {
    await renderReady()
    const select = screen.getByRole('combobox', { name: 'Account' }) as HTMLSelectElement

    await userEvent.selectOptions(select, select.options[1]?.value ?? '')

    await waitFor(() => {
      expect(api.entryList).toHaveBeenLastCalledWith('e1', expect.objectContaining({ accountId: select.options[1]?.value }))
    })
    expect(api.cashFlowSeries).toHaveBeenLastCalledWith('e1', null, null)
    expect(screen.getByText("The account and search filters don’t change this summary.")).toBeInTheDocument()
  })

  test('an inverted date range draws nothing and says why', async () => {
    vi.mocked(api.cashFlowSeries).mockImplementation(async (_entityId, from, to) => {
      if (from && to) {
        throw { code: 'date_range_inverted', message: 'from date must be on or before to' }
      }
      return series
    })
    await renderReady()

    await userEvent.type(screen.getByLabelText('Filter from date'), '31/08/2026')
    await userEvent.tab()
    await userEvent.type(screen.getByLabelText('Filter to date'), '01/08/2026')
    await userEvent.tab()

    expect(await screen.findByText('The From date must be on or before the To date.')).toBeInTheDocument()
    const alert = screen.queryByRole('alert')
    expect(alert?.textContent ?? '').not.toContain('from date must be on or before to')
  })

  test('a failing summary never stops the list from refreshing', async () => {
    vi.mocked(api.cashFlowSeries).mockRejectedValue({ code: 'io', message: 'disk' })
    vi.mocked(api.entryList).mockResolvedValue([postedEntry, incomeEntry])
    await renderReady()

    await waitFor(() => {
      expect(screen.getByText('Alpha supermarket')).toBeTruthy()
      expect(screen.getByText('CLIENT INVOICE')).toBeTruthy()
    })

    const heading = await screen.findByRole('heading', { name: 'In view' })
    const pane = heading.closest('section')
    await waitFor(() => {
      expect(pane?.querySelector('[data-net]')).toHaveTextContent('—')
    })
    expect(pane).toHaveTextContent(commandErrorMessage({ code: 'io', message: 'disk' }))
  })

  test('a failing summary with an unknown code never shows the raw message', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.cashFlowSeries).mockRejectedValue({
      code: 'brand_new',
      message: 'sqlcipher: disk image is malformed',
    })
    await renderReady()

    const heading = await screen.findByRole('heading', { name: 'In view' })
    const pane = heading.closest('section')
    await waitFor(() => {
      expect(pane).toHaveTextContent('Something went wrong.')
    })
    expect(pane).not.toHaveTextContent('sqlcipher')
  })
})

describe('TransactionsPage New entry type colours', () => {
  test('the chosen type wears its Ledger gradient; Transfer stays neutral', async () => {
    await renderReady()
    await userEvent.click(screen.getByRole('button', { name: 'New Entry' }))
    await screen.findByRole('heading', { name: 'New entry' })

    expect(screen.getByRole('button', { name: 'Expense' })).toHaveAttribute('data-tone', 'money-out')

    await userEvent.click(screen.getByRole('button', { name: 'Income' }))
    expect(screen.getByRole('button', { name: 'Income' })).toHaveAttribute('data-tone', 'money-in')

    await userEvent.click(screen.getByRole('button', { name: 'Bill' }))
    expect(screen.getByRole('button', { name: 'Bill' })).toHaveAttribute('data-tone', 'money-out')

    await userEvent.click(screen.getByRole('button', { name: 'Transfer' }))
    expect(screen.getByRole('button', { name: 'Transfer' })).toHaveAttribute('data-tone', 'neutral')
  })
})
