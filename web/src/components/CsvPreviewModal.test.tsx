/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { CsvPreviewModal } from './CsvPreviewModal'
import type { Account, CsvImportPreview, SimpleEntryInput } from '../lib/api'
import { resetI18nForTests, setLocale, type Locale } from '../lib/i18n'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

function previewWith(error: { code: string; params?: Record<string, string> }): CsvImportPreview {
  return {
    source: 'bank.csv',
    headers: ['Date', 'Description', 'Amount'].map((name) => ({ name })),
    rows: [
      {
        source_row: 2,
        duplicate: false,
        error,
        suggested: null,
        signed_amount_minor: null,
      },
    ],
  }
}

function renderPreview(preview: CsvImportPreview) {
  render(
    <CsvPreviewModal
      open
      preview={preview}
      currency={{ code: 'EUR', decimals: 2 }}
      walletAccounts={[]}
      expenseAccounts={[]}
      incomeAccounts={[]}
      onClose={() => {}}
      onConfirm={() => {}}
    />,
  )
}

describe('CsvPreviewModal currency warning', () => {
  test('warns when the amounts carry another currency than the book', () => {
    renderPreview({ ...previewWith({ code: 'csv_zero_amount' }), other_currency: 'HUF' })

    expect(screen.getByRole('alert')).toHaveTextContent(
      'Some amounts are marked “HUF”, but this book is in EUR.',
    )
  })

  test('says nothing when no amount carries another currency', () => {
    renderPreview({ ...previewWith({ code: 'csv_zero_amount' }), other_currency: null })

    expect(screen.queryByRole('alert')).toBeNull()
  })
})

describe('CsvPreviewModal row problems', () => {
  const cases: Array<[Locale, string]> = [
    ['en', '“not-a-date” is not a valid date.'],
    ['el', 'Το «not-a-date» δεν είναι έγκυρη ημερομηνία.'],
    ['fr', '« not-a-date » n’est pas une date valide.'],
    ['de', '„not-a-date“ ist kein gültiges Datum.'],
  ]

  test.each(cases)('shows the %s sentence with the offending value', (locale, sentence) => {
    setLocale(locale)
    renderPreview(previewWith({ code: 'csv_invalid_date', params: { value: 'not-a-date' } }))

    // Keep the narrow no-break spaces French puts inside guillemets.
    expect(screen.getByText(sentence, { normalizer: (text) => text })).toBeInTheDocument()
    expect(screen.queryByText('csv_invalid_date')).toBeNull()
  })

  test('a row without a value shows its fixed sentence in German', () => {
    setLocale('de')
    renderPreview(previewWith({ code: 'csv_zero_amount' }))

    expect(screen.getByText('Der Betrag ist null.')).toBeInTheDocument()
  })

  test('never shows the parser English text in another language', () => {
    setLocale('fr')
    renderPreview(previewWith({ code: 'csv_zero_amount' }))

    expect(screen.queryByText('amount is zero')).toBeNull()
    expect(screen.getByText('Le montant est nul.')).toBeInTheDocument()
  })

  test('an unknown code falls back to the unreadable-row sentence, never the code', () => {
    setLocale('de')
    renderPreview(previewWith({ code: 'csv_from_the_future' }))

    expect(screen.getByText('Diese Zeile konnte nicht gelesen werden.')).toBeInTheDocument()
    expect(screen.queryByText('csv_from_the_future')).toBeNull()
  })
})

function account(id: string, name: string, accountType: 'expense' | 'income'): Account {
  return {
    id,
    entity_id: 'e1',
    code: id,
    name,
    account_type: accountType,
    is_active: true,
    is_system: false,
    sort_order: 0,
  }
}

const expenseAccounts = [account('exp1', 'Groceries', 'expense'), account('exp2', 'Rent', 'expense')]
const incomeAccounts = [account('inc1', 'Salary', 'income'), account('inc2', 'Interest', 'income')]

function suggestedRow(
  sourceRow: number,
  kind: 'expense' | 'income',
  categoryId: string | null = kind === 'expense' ? 'exp1' : 'inc1',
): CsvImportPreview['rows'][number] {
  return {
    source_row: sourceRow,
    duplicate: false,
    error: null,
    suggested: {
      entity_id: 'e1',
      kind,
      bill_status: null,
      entry_date: '2026-08-12',
      description: `Row ${sourceRow}`,
      reference: null,
      amount_minor: 4250,
      category_account_id: categoryId,
      wallet_account_id: 'w1',
      payable_account_id: null,
      from_account_id: null,
      to_account_id: null,
    },
    signed_amount_minor: kind === 'expense' ? -4250 : 4250,
  }
}

const mixedRows = [suggestedRow(2, 'expense'), suggestedRow(3, 'expense'), suggestedRow(4, 'income')]

function renderCategoryPreview(
  rows: CsvImportPreview['rows'],
  onConfirm: (input: { rows: SimpleEntryInput[] }) => void = () => {},
) {
  render(
    <CsvPreviewModal
      open
      preview={{ source: 'bank.csv', headers: ['Date', 'Description', 'Amount'], rows }}
      currency={{ code: 'EUR', decimals: 2 }}
      walletAccounts={[]}
      expenseAccounts={expenseAccounts}
      incomeAccounts={incomeAccounts}
      onClose={() => {}}
      onConfirm={onConfirm}
    />,
  )
}

function rowCategory(n: number): HTMLSelectElement {
  return screen.getByRole('combobox', { name: `Category for row ${n}` })
}

function optionNames(select: HTMLSelectElement): string[] {
  return within(select)
    .getAllByRole('option')
    .map((option) => option.textContent)
}

describe('CsvPreviewModal category column', () => {
  test('shows the category each row posts to', () => {
    renderCategoryPreview(mixedRows)

    expect(rowCategory(2)).toHaveDisplayValue('Groceries')
    expect(rowCategory(4)).toHaveDisplayValue('Salary')
  })

  test('a truncated description keeps its full text on hover', () => {
    renderCategoryPreview(mixedRows)

    expect(screen.getByText('Row 2')).toHaveAttribute('title', 'Row 2')
  })

  test('lists only the accounts of the row kind', () => {
    renderCategoryPreview(mixedRows)

    expect(optionNames(rowCategory(2))).toEqual(['Groceries', 'Rent'])
    expect(optionNames(rowCategory(4))).toEqual(['Salary', 'Interest'])
  })

  test('a row pick changes only that row and is what gets posted', () => {
    const onConfirm = vi.fn<(input: { rows: SimpleEntryInput[] }) => void>()
    renderCategoryPreview(mixedRows, onConfirm)

    fireEvent.change(rowCategory(2), { target: { value: 'exp2' } })

    expect(rowCategory(2)).toHaveDisplayValue('Rent')
    expect(rowCategory(3)).toHaveDisplayValue('Groceries')
    fireEvent.click(screen.getByRole('button', { name: 'Post 3 selected' }))
    const posted = onConfirm.mock.calls[0]?.[0].rows.map((row) => row.category_account_id)
    expect(posted).toEqual(['exp2', 'exp1', 'inc1'])
  })

  test('a new preview replaces the rows and the picks made on the last one', () => {
    const modal = (rows: CsvImportPreview['rows']) => (
      <CsvPreviewModal
        open
        preview={{ source: 'bank.csv', headers: ['Date', 'Description', 'Amount'], rows }}
        currency={{ code: 'EUR', decimals: 2 }}
        walletAccounts={[]}
        expenseAccounts={expenseAccounts}
        incomeAccounts={incomeAccounts}
        onClose={() => {}}
        onConfirm={() => {}}
      />
    )
    const { rerender } = render(modal(mixedRows))
    fireEvent.change(rowCategory(2), { target: { value: 'exp2' } })

    rerender(modal([suggestedRow(2, 'expense'), suggestedRow(5, 'expense')]))

    expect(rowCategory(2)).toHaveDisplayValue('Groceries')
    expect(rowCategory(5)).toHaveDisplayValue('Groceries')
    expect(screen.queryByRole('combobox', { name: 'Category for row 4' })).toBeNull()
  })

  test('a re-render with the same preview keeps the picks', () => {
    const preview = { source: 'bank.csv', headers: ['Date', 'Description', 'Amount'], rows: mixedRows }
    // The parent keeps one identity for its account lists, as TransactionsPage does.
    const wallets: Account[] = []
    const modal = (busy: boolean) => (
      <CsvPreviewModal
        open
        preview={preview}
        currency={{ code: 'EUR', decimals: 2 }}
        walletAccounts={wallets}
        expenseAccounts={expenseAccounts}
        incomeAccounts={incomeAccounts}
        busy={busy}
        onClose={() => {}}
        onConfirm={() => {}}
      />
    )
    const { rerender } = render(modal(false))
    fireEvent.change(rowCategory(2), { target: { value: 'exp2' } })

    rerender(modal(true))

    expect(rowCategory(2)).toHaveDisplayValue('Rent')
  })

  test('apply to selected shows in the column, for the matching kind only', () => {
    renderCategoryPreview(mixedRows)

    fireEvent.change(screen.getByRole('combobox', { name: 'Bulk category' }), {
      target: { value: 'exp2' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Apply to selected' }))

    expect(rowCategory(2)).toHaveDisplayValue('Rent')
    expect(rowCategory(3)).toHaveDisplayValue('Rent')
    expect(rowCategory(4)).toHaveDisplayValue('Salary')
  })

  test('a row whose category is not listed opens on an empty choice', () => {
    renderCategoryPreview([suggestedRow(2, 'expense', null), suggestedRow(3, 'income', 'gone')])

    expect(rowCategory(2)).toHaveDisplayValue('Choose a category')
    expect(rowCategory(3)).toHaveDisplayValue('Choose a category')
  })
})

describe('CsvPreviewModal rows that need a category', () => {
  const postButton = (n: number) => screen.getByRole('button', { name: `Post ${n} selected` })

  test('flags each such row and keeps Import disabled, saying why', () => {
    const onConfirm = vi.fn<(input: { rows: SimpleEntryInput[] }) => void>()
    renderCategoryPreview(
      [
        suggestedRow(2, 'expense', null),
        suggestedRow(3, 'income', 'gone'),
        suggestedRow(4, 'expense'),
      ],
      onConfirm,
    )

    expect(screen.getAllByText('Needs a category')).toHaveLength(2)
    expect(postButton(3)).toBeDisabled()
    expect(postButton(3)).toHaveAccessibleDescription(
      'Choose a category for 2 selected rows before you import.',
    )
    fireEvent.click(postButton(3))
    expect(onConfirm).not.toHaveBeenCalled()
  })

  test('an income row pointed at an expense account needs a category too', () => {
    renderCategoryPreview([suggestedRow(2, 'income', 'exp1')])

    expect(screen.getByText('Needs a category')).toBeInTheDocument()
    expect(
      screen.getByText('Choose a category for 1 selected row before you import.'),
    ).toBeInTheDocument()
  })

  test('choosing a category for each row enables Import and clears the message', () => {
    const onConfirm = vi.fn<(input: { rows: SimpleEntryInput[] }) => void>()
    renderCategoryPreview([suggestedRow(2, 'expense', null), suggestedRow(3, 'expense')], onConfirm)

    fireEvent.change(rowCategory(2), { target: { value: 'exp2' } })

    expect(screen.queryByText('Needs a category')).toBeNull()
    expect(screen.queryByText(/before you import/)).toBeNull()
    expect(postButton(2)).toBeEnabled()
    fireEvent.click(postButton(2))
    expect(onConfirm).toHaveBeenCalledOnce()
  })

  test('a row left out of the import does not block it, but stays flagged', () => {
    renderCategoryPreview([suggestedRow(2, 'expense', null), suggestedRow(3, 'expense')])

    fireEvent.click(screen.getByRole('checkbox', { name: 'Select row 2' }))

    expect(screen.getByText('Needs a category')).toBeInTheDocument()
    expect(postButton(1)).toBeEnabled()
  })

  test('says so in Greek', () => {
    setLocale('el')
    renderCategoryPreview([suggestedRow(2, 'expense', null)])

    expect(screen.getByText('Χρειάζεται κατηγορία')).toBeInTheDocument()
    expect(
      screen.getByText('Επιλέξτε κατηγορία για 1 επιλεγμένη γραμμή πριν από την εισαγωγή.'),
    ).toBeInTheDocument()
  })
})
