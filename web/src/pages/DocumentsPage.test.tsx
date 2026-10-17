/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return { ...actual, api: { ...actual.api, documentList: vi.fn() } }
})

import { api, type DocumentMeta, type Entity } from '../lib/api'
import { applyLocale, resetI18nForTests, type Locale } from '../lib/i18n'
import { DocumentsPage } from './DocumentsPage'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

describe('DocumentsPage empty-state CTA', () => {
  test('no-book empty state renders Create a book and calls onCreateBook', async () => {
    const onCreateBook = vi.fn()
    render(<DocumentsPage entity={null} onCreateBook={onCreateBook} />)
    const cta = screen.getByRole('button', { name: 'Create a book' })
    await userEvent.click(cta)
    expect(onCreateBook).toHaveBeenCalledTimes(1)
  })

  test('renders without a CTA when onCreateBook is not supplied', () => {
    render(<DocumentsPage entity={null} />)
    expect(screen.queryByRole('button', { name: 'Create a book' })).toBeNull()
  })
})

describe('DocumentsPage dates', () => {
  const entity: Entity = {
    id: 'e1',
    name: 'Personal',
    base_currency: 'EUR',
    base_currency_decimals: 2,
    fiscal_year_start_month: 1,
    chart_template: 'personal',
  }

  // Noon local time, so the day is 16 October in every time zone the test runs in.
  const savedAt = new Date(2026, 9, 16, 12).getTime() / 1000

  const receipt: DocumentMeta = {
    id: 'd1',
    entity_id: 'e1',
    entry_id: 'j1',
    filename: 'receipt.pdf',
    mime_type: 'application/pdf',
    size_bytes: 5,
    created_at: `unix:${String(savedAt)}`,
    entry_description: 'Rent',
  }

  test.each<[Locale, string]>([
    ['en', '16/10/2026'],
    ['el', '16/10/2026'],
    ['fr', '16/10/2026'],
    ['de', '16.10.2026'],
  ])('%s shows the day a file was saved as %s', async (locale, shown) => {
    applyLocale(locale)
    vi.mocked(api.documentList).mockResolvedValue([receipt])

    render(<DocumentsPage entity={entity} />)

    // The date shares its line with the file size, so match the line's start.
    const line = await screen.findByText(
      (_, element) => element?.firstChild?.textContent === shown,
    )
    expect(line).toBeInTheDocument()
  })
})
