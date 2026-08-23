/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

import { invoke } from '@tauri-apps/api/core'
import { api, type JournalEntry, type PostedEntryView } from './api'

const posted: PostedEntryView = {
  entry: {
    id: 'j1',
    entity_id: 'e1',
    entry_date: '2026-08-12',
    description: 'Alpha supermarket',
    reference: null,
    status: 'posted',
    hidden: false,
  },
  lines: [],
  is_voided: false,
}

beforeEach(() => {
  Object.defineProperty(window, '__TAURI_INTERNALS__', {
    value: {},
    configurable: true,
  })
  vi.mocked(invoke).mockReset()
})

afterEach(() => {
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

describe('hidden on list/get types', () => {
  test('JournalEntry and PostedEntryView require hidden', () => {
    const entry: JournalEntry = posted.entry
    const view: PostedEntryView = posted
    expect(entry.hidden).toBe(false)
    expect(view.entry.hidden).toBe(false)
    expect(typeof view.entry.hidden).toBe('boolean')
  })

  test('entry_list returns hidden on each PostedEntryView', async () => {
    const hiddenRow: PostedEntryView = {
      ...posted,
      entry: { ...posted.entry, hidden: true },
    }
    vi.mocked(invoke).mockResolvedValue([posted, hiddenRow])

    const list = await api.entryList('e1')
    expect(list.map((row) => row.entry.hidden)).toEqual([false, true])
    expect(invoke).toHaveBeenCalledWith('entry_list', {
      entityId: 'e1',
      from: null,
      to: null,
      search: null,
      accountId: null,
    })
  })
})

describe('entry_set_hidden', () => {
  test('is called with { id, hidden }', async () => {
    const hidden: PostedEntryView = {
      ...posted,
      entry: { ...posted.entry, hidden: true },
    }
    vi.mocked(invoke).mockResolvedValue(hidden)

    const result = await api.entrySetHidden('j1', true)
    expect(result.entry.hidden).toBe(true)
    expect(invoke).toHaveBeenCalledWith('entry_set_hidden', { id: 'j1', hidden: true })

    vi.mocked(invoke).mockResolvedValue(posted)
    await api.entrySetHidden('j1', false)
    expect(invoke).toHaveBeenCalledWith('entry_set_hidden', { id: 'j1', hidden: false })
  })
})

describe('report PDF export path', () => {
  test('reportExportPdf invokes report_export_pdf with camelCase args', async () => {
    vi.mocked(invoke).mockResolvedValue('/tmp/oikonomia-expenses.pdf')
    await api.reportExportPdf({
      bytesBase64: 'JVBERi0x',
      suggestedName: 'oikonomia-expenses-2026-08-01_2026-08-31.pdf',
    })
    expect(invoke).toHaveBeenCalledWith('report_export_pdf', {
      bytesBase64: 'JVBERi0x',
      suggestedName: 'oikonomia-expenses-2026-08-01_2026-08-31.pdf',
    })
  })

  test('maps cancel to null', async () => {
    vi.mocked(invoke).mockResolvedValue(null)
    expect(await api.reportExportPdf({ bytesBase64: 'JVBERi0x' })).toBeNull()
  })
})

describe('csv journal export path', () => {
  test('csvExportJournal calls csv_export_journal with entityId only', async () => {
    vi.mocked(invoke).mockResolvedValue('/tmp/journal.csv')
    await api.csvExportJournal('e1')
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith('csv_export_journal', { entityId: 'e1' })
    const args = vi.mocked(invoke).mock.calls[0]?.[1] as Record<string, unknown>
    expect(args).not.toHaveProperty('includeHidden')
    expect(args).not.toHaveProperty('include_hidden')
    expect(args).not.toHaveProperty('hidden')
  })
})
