/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return { ...actual, api: { ...actual.api, entryHistory: vi.fn() } }
})

import { api, type EntryHistoryItem } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { EntryHistory } from './EntryHistory'

beforeEach(() => {
  vi.mocked(api.entryHistory).mockReset()
})

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

const items: EntryHistoryItem[] = [
  { entry_id: 'a', entry_date: '2026-03-15', description: 'lunch', change: 'original' },
  { entry_id: 'b', entry_date: '2026-03-15', description: 'VOID: lunch', change: 'reversal' },
]

describe('EntryHistory', () => {
  test('lists each stored entry with its kind of change and description', async () => {
    vi.mocked(api.entryHistory).mockResolvedValue(items)

    render(<EntryHistory entryId="c" />)

    const list = await screen.findByRole('list')
    const rows = within(list).getAllByRole('listitem')
    expect(rows).toHaveLength(2)
    expect(rows[0]).toHaveTextContent('Original entry')
    expect(rows[0]).toHaveTextContent('lunch')
    expect(rows[1]).toHaveTextContent('Reversal')
    // The stored text is shown as core wrote it, not reworded by the UI.
    expect(rows[1]).toHaveTextContent('VOID: lunch')
    expect(screen.getByRole('heading', { name: 'History' })).toBeInTheDocument()
    expect(api.entryHistory).toHaveBeenCalledWith('c')
  })

  test('shows nothing for an entry that was never edited', async () => {
    vi.mocked(api.entryHistory).mockResolvedValue([])

    const { container } = render(<EntryHistory entryId="c" />)

    await vi.waitFor(() => expect(api.entryHistory).toHaveBeenCalled())
    expect(container).toBeEmptyDOMElement()
  })

  test('says so, in its own words, when the history cannot be loaded', async () => {
    vi.mocked(api.entryHistory).mockRejectedValue({ code: 'database', message: 'raw backend text' })

    render(<EntryHistory entryId="c" />)

    expect(await screen.findByText('Could not load the history.')).toBeInTheDocument()
    expect(screen.queryByText(/raw backend text/)).toBeNull()
  })
})
