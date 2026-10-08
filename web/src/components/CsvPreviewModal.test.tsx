/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, test } from 'vitest'

import { CsvPreviewModal } from './CsvPreviewModal'
import type { CsvImportPreview } from '../lib/api'
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
