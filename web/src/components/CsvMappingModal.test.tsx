/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useLayoutEffect } from 'react'
import { afterEach, describe, expect, test } from 'vitest'

import { CsvMappingModal } from './CsvMappingModal'
import type { CsvImportPreview } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

const DETECTED: CsvImportPreview = {
  source: 'first.csv',
  headers: [{ name: 'Date' }, { name: 'Text' }, { name: 'Amount' }],
  detected_mapping: { date: 'Date', description: 'Text', amount: 'Amount' },
  rows: [],
}

const AMOUNT_UNDETECTED: CsvImportPreview = {
  source: 'second.csv',
  headers: [{ name: 'Date' }, { name: 'Memo' }, { name: 'Value' }],
  detected_mapping: { date: 'Date', description: 'Memo' },
  missing_columns: ['amount'],
  rows: [],
}

function amountSelect(): HTMLSelectElement {
  return screen.getByRole('combobox', { name: 'Amount source column' })
}

function continueButton(): HTMLElement {
  return screen.getByRole('button', { name: 'Continue to preview' })
}

function modal(open: boolean, preview: CsvImportPreview | null) {
  return <CsvMappingModal open={open} preview={preview} onClose={() => {}} onContinue={() => {}} />
}

describe('CsvMappingModal required fields', () => {
  test('an undetected amount column shows Not mapped', () => {
    render(modal(true, AMOUNT_UNDETECTED))

    expect(amountSelect()).toHaveValue('')
    expect(amountSelect().selectedOptions[0]).toHaveTextContent('Not mapped')
    expect(continueButton()).toBeDisabled()
  })

  test('a mapped required field can be set back to Not mapped, which blocks Continue', async () => {
    const user = userEvent.setup()
    render(modal(true, DETECTED))
    expect(continueButton()).toBeEnabled()

    await user.selectOptions(amountSelect(), 'Not mapped')

    expect(amountSelect()).toHaveValue('')
    expect(continueButton()).toBeDisabled()
    expect(
      screen.getByText('Choose an amount column, or a debit or a credit column.'),
    ).toBeInTheDocument()
  })
})

describe('CsvMappingModal between imports', () => {
  test("opening a second file never shows the first file's mapping", () => {
    // The dialog stays mounted between imports, so the first frame after
    // reopening is where a stale draft would show. A layout effect sees that
    // frame before any passive effect can correct it.
    const firstFrame: string[] = []
    function Probe({ open }: { open: boolean }) {
      useLayoutEffect(() => {
        if (open) firstFrame.push(amountSelect().value)
      }, [open])
      return null
    }

    const { rerender } = render(
      <>
        {modal(true, DETECTED)}
        <Probe open />
      </>,
    )
    rerender(
      <>
        {modal(false, null)}
        <Probe open={false} />
      </>,
    )
    rerender(
      <>
        {modal(true, AMOUNT_UNDETECTED)}
        <Probe open />
      </>,
    )

    expect(firstFrame).toEqual(['Amount', ''])
    expect(amountSelect()).toHaveValue('')
    // A stale draft can hide behind the empty option and still map the
    // first file's column; only a fresh draft leaves Continue blocked.
    expect(continueButton()).toBeDisabled()
  })

  test('reopening after a cancelled edit starts from the detected mapping', async () => {
    const user = userEvent.setup()
    const { rerender } = render(modal(true, DETECTED))
    await user.selectOptions(amountSelect(), 'Text')

    rerender(modal(false, null))
    rerender(modal(true, { ...DETECTED }))

    expect(amountSelect()).toHaveValue('Amount')
  })
})
