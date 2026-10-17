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
    headers: ['Date', 'Description', 'Amount'],
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
    ['en', '"not-a-date" is not a valid date.'],
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
