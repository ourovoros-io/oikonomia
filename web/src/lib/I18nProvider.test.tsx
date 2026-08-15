/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('./api', () => ({
  api: {
    getLocale: vi.fn(),
    setLocale: vi.fn(),
    getUiPrefs: vi.fn(),
  },
}))

import { api } from './api'
import { I18nProvider, useI18n } from './I18nProvider'
import { getLocale, resetI18nForTests, setLocale } from './i18n'

function Probe() {
  const { locale, t } = useI18n()
  return (
    <div>
      <span>{locale}</span>
      <h1>{t('settings.title')}</h1>
    </div>
  )
}

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(api.getLocale).mockReset()
  vi.mocked(api.setLocale).mockReset()
  vi.mocked(api.getUiPrefs).mockReset()
})

describe('I18nProvider', () => {
  test('hydrates locale from settings_get_locale (prefs win over cache)', async () => {
    localStorage.setItem('oikonomia.locale', 'en')
    vi.mocked(api.getLocale).mockResolvedValue('el')
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(getLocale()).toBe('el')
      expect(screen.getByText('el')).toBeTruthy()
    })
  })

  test('falls back to cache when locale commands fail', async () => {
    localStorage.setItem('oikonomia.locale', 'el')
    vi.mocked(api.getLocale).mockRejectedValue(new Error('no tauri'))
    vi.mocked(api.getUiPrefs).mockRejectedValue(new Error('no tauri'))
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(getLocale()).toBe('el')
    })
  })

  test('setLocale persists via settings_set_locale', async () => {
    vi.mocked(api.getLocale).mockResolvedValue('en')
    vi.mocked(api.setLocale).mockResolvedValue(undefined)
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(api.getLocale).toHaveBeenCalled()
    })
    setLocale('el')
    await waitFor(() => {
      expect(api.setLocale).toHaveBeenCalledWith('el')
    })
  })
})
