/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      documentAnalyzerStatus: vi.fn(),
      documentAnalyze: vi.fn(),
      documentGet: vi.fn(),
      documentExport: vi.fn(),
    },
  }
})

import { api } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { DocumentDropZone } from './DocumentDropZone'
import { DocumentViewerModal } from './DocumentViewerModal'

const RAW = 'sqlcipher: disk image is malformed'

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined)
  vi.mocked(api.documentAnalyzerStatus)
    .mockReset()
    .mockResolvedValue({ ocr_available: true, offline: true, hint: 'ready' })
})

afterEach(() => {
  cleanup()
  resetI18nForTests()
  vi.restoreAllMocks()
})

async function dropFile(container: HTMLElement) {
  const input = container.querySelector('input[type=file]') as HTMLInputElement
  const file = new File(['hello'], 'note.txt', { type: 'text/plain' })

  await userEvent.upload(input, file)
}

describe('DocumentDropZone errors', () => {
  test('a known code shows the localized copy with its parameters', async () => {
    vi.mocked(api.documentAnalyze).mockReset().mockRejectedValue({
      code: 'file_too_large',
      message: 'file too large (max 8 MB)',
      params: { max_mb: '8' },
    })
    const onError = vi.fn()
    const { container } = render(
      <DocumentDropZone entityId="e1" onSuggestion={vi.fn()} onError={onError} />,
    )

    await dropFile(container)

    await waitFor(() => {
      expect(onError).toHaveBeenCalledWith('That file is too large. The limit is 8 MB.')
    })
  })

  test('an unknown code shows the analyze fallback, never the raw message', async () => {
    vi.mocked(api.documentAnalyze).mockReset().mockRejectedValue({ code: 'brand_new', message: RAW })
    const onError = vi.fn()
    const { container } = render(
      <DocumentDropZone entityId="e1" onSuggestion={vi.fn()} onError={onError} />,
    )

    await dropFile(container)

    await waitFor(() => {
      expect(onError).toHaveBeenCalledWith('Could not analyze the document.')
    })
    expect(screen.getByText('Could not analyze the document.')).toBeInTheDocument()
    expect(screen.queryByText(RAW)).toBeNull()
  })

  test('a failed file read is localized like any other error', async () => {
    // A FileReader that always fails, as a browser does for an unreadable file.
    class FailingReader {
      onload: (() => void) | null = null
      onerror: (() => void) | null = null
      result = null

      readAsDataURL() {
        queueMicrotask(() => this.onerror?.())
      }
    }
    vi.stubGlobal('FileReader', FailingReader)
    vi.mocked(api.documentAnalyze).mockReset()
    const onError = vi.fn()
    const { container } = render(
      <DocumentDropZone entityId="e1" onSuggestion={vi.fn()} onError={onError} />,
    )

    await dropFile(container)

    await waitFor(() => {
      expect(onError).toHaveBeenCalledWith('Could not read the file')
    })
    expect(api.documentAnalyze).not.toHaveBeenCalled()
    vi.unstubAllGlobals()
  })
})

const meta = {
  id: 'd1',
  entity_id: 'e1',
  entry_id: 'j1',
  filename: 'note.txt',
  mime_type: 'text/plain',
  size_bytes: 5,
  created_at: '2026-08-01',
  entry_description: 'Rent',
}

describe('DocumentViewerModal errors', () => {
  test('a failed open with an unknown code shows the open fallback, never the raw message', async () => {
    vi.mocked(api.documentGet).mockReset().mockRejectedValue({ code: 'brand_new', message: RAW })
    const onError = vi.fn()

    render(<DocumentViewerModal documentId="d1" onClose={vi.fn()} onError={onError} />)

    await waitFor(() => {
      expect(onError).toHaveBeenCalledWith('Could not open the document.')
    })
    expect(onError).not.toHaveBeenCalledWith(RAW)
  })

  test('a failed save of a copy shows the copy for its code', async () => {
    vi.mocked(api.documentGet)
      .mockReset()
      .mockResolvedValue({ meta, data_base64: 'aGVsbG8=' })
    vi.mocked(api.documentExport).mockReset().mockRejectedValue({
      code: 'save_failed',
      message: 'Permission denied (os error 13)',
    })
    const onError = vi.fn()

    render(<DocumentViewerModal documentId="d1" onClose={vi.fn()} onError={onError} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Save a copy' }))

    await waitFor(() => {
      expect(onError).toHaveBeenCalledWith(
        'Could not save the file. Check that the location is writable and try again.',
      )
    })
  })

  test('a failed save of a copy with an unknown code shows the save fallback', async () => {
    vi.mocked(api.documentGet)
      .mockReset()
      .mockResolvedValue({ meta, data_base64: 'aGVsbG8=' })
    vi.mocked(api.documentExport).mockReset().mockRejectedValue({ code: 'brand_new', message: RAW })
    const onError = vi.fn()

    render(<DocumentViewerModal documentId="d1" onClose={vi.fn()} onError={onError} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Save a copy' }))

    await waitFor(() => {
      expect(onError).toHaveBeenCalledWith('Could not save a copy.')
    })
  })
})
