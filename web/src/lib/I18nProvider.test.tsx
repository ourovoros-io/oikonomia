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
import { LOCALE_STORAGE_KEY, getLocale, resetI18nForTests, setLocale } from './i18n'

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
    localStorage.setItem(LOCALE_STORAGE_KEY, 'en')
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
    expect(screen.getByRole('heading', { name: 'Ρυθμίσεις' })).toBeTruthy()
  })

  test('falls back to cache when locale commands fail', async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, 'el')
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

  test('falls back to getUiPrefs().locale when settings_get_locale fails', async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, 'en')
    vi.mocked(api.getLocale).mockRejectedValue(new Error('no getLocale'))
    vi.mocked(api.getUiPrefs).mockResolvedValue({
      theme: 'dark',
      last_entity_id: null,
      last_accounts_by_entity_kind: {},
      locale: 'el',
    })
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(getLocale()).toBe('el')
    })
  })

  test('setLocale writes settings_set_locale and mirrors oikonomia.locale', async () => {
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
      expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe('el')
    })
  })

  test('last-used fr persists on hydrate from getLocale', async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, 'en')
    vi.mocked(api.getLocale).mockResolvedValue('fr')
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(getLocale()).toBe('fr')
      expect(screen.getByText('fr')).toBeTruthy()
    })
  })

  test('last-used de persists on hydrate from prefs when getLocale fails', async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, 'en')
    vi.mocked(api.getLocale).mockRejectedValue(new Error('no getLocale'))
    vi.mocked(api.getUiPrefs).mockResolvedValue({
      theme: 'dark',
      last_entity_id: null,
      last_accounts_by_entity_kind: {},
      locale: 'de',
    })
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(getLocale()).toBe('de')
    })
  })

  test('last-used locale persists on hydrate from localStorage when commands fail', async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, 'fr')
    vi.mocked(api.getLocale).mockRejectedValue(new Error('no tauri'))
    vi.mocked(api.getUiPrefs).mockRejectedValue(new Error('no tauri'))
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(getLocale()).toBe('fr')
    })
  })
})
