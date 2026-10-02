/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: { ...actual.api, documentAnalyzerStatus: vi.fn() },
  }
})

import { api } from '../lib/api'
import { resetI18nForTests, setLocale } from '../lib/i18n'
import { DocumentDropZone } from './DocumentDropZone'

beforeEach(() => {
  resetI18nForTests()
})

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

function renderZone() {
  return render(<DocumentDropZone entityId="e1" onSuggestion={vi.fn()} onError={vi.fn()} />)
}

describe('DocumentDropZone status hint', () => {
  test('the ready hint is shown in the current language', async () => {
    setLocale('el')
    vi.mocked(api.documentAnalyzerStatus).mockResolvedValue({
      ocr_available: true,
      offline: true,
      hint: 'ready',
    })

    renderZone()

    await waitFor(() => {
      expect(
        screen.getByText(
          'Ενσωματωμένος τοπικός αναγνώστης τιμολογίων + OCR — τίποτα δεν φεύγει από αυτή τη συσκευή.',
        ),
      ).toBeTruthy()
    })
  })

  test('the missing-models hint is shown in German', async () => {
    setLocale('de')
    vi.mocked(api.documentAnalyzerStatus).mockResolvedValue({
      ocr_available: false,
      offline: true,
      hint: 'models_missing',
    })

    renderZone()

    await waitFor(() => {
      expect(
        screen.getByText(
          'Im App-Paket fehlen OCR-Modelle. Text-PDFs werden weiterhin vom Offline-Rechnungsleser gelesen.',
        ),
      ).toBeTruthy()
    })
  })

  test('English shows the English sentence', async () => {
    vi.mocked(api.documentAnalyzerStatus).mockResolvedValue({
      ocr_available: false,
      offline: true,
      hint: 'models_missing',
    })

    renderZone()

    await waitFor(() => {
      expect(
        screen.getByText(
          'OCR models missing from the app bundle. Text PDFs still use the offline invoice reader.',
        ),
      ).toBeTruthy()
    })
  })

  test('an unreadable status says so instead of showing a code', async () => {
    vi.mocked(api.documentAnalyzerStatus).mockRejectedValue(new Error('boom'))

    renderZone()

    await waitFor(() => {
      expect(screen.getByText('Built-in analyzer status unavailable.')).toBeTruthy()
    })
  })
})
