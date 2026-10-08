/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeAll, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return { ...actual, api: { ...actual.api, documentGet: vi.fn(), documentExport: vi.fn() } }
})

import { api, type DocumentMeta } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { DocumentViewerModal } from './DocumentViewerModal'

beforeAll(() => {
  // jsdom has no object URLs; the viewer makes one for each document.
  URL.createObjectURL = vi.fn(() => 'blob:test')
  URL.revokeObjectURL = vi.fn()
})

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

function meta(filename: string, mime_type: string): DocumentMeta {
  return {
    id: 'd1',
    entity_id: 'e1',
    entry_id: 'j1',
    filename,
    mime_type,
    size_bytes: 5,
    created_at: '2026-03-15',
    entry_description: 'Rent',
  }
}

function open(filename: string, mimeType: string) {
  vi.mocked(api.documentGet).mockResolvedValue({
    meta: meta(filename, mimeType),
    data_base64: btoa('hello'),
  })
  render(<DocumentViewerModal documentId="d1" onClose={() => {}} onError={() => {}} />)
}

describe('DocumentViewerModal', () => {
  test('a PDF shows the no-preview message and Save a copy, not a blank frame', async () => {
    open('statement.pdf', 'application/pdf')

    expect(
      await screen.findByText(/No in-app preview for application\/pdf/),
    ).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Save a copy/ })).toBeInTheDocument()
    expect(document.querySelector('iframe')).toBeNull()
  })

  test('an image is still previewed', async () => {
    open('scan.png', 'image/png')

    expect(await screen.findByRole('img', { name: 'scan.png' })).toBeInTheDocument()
  })
})
