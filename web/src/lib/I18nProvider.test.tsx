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

  test('keeps document.documentElement.lang equal to the active locale', async () => {
    // CSS uppercase (font-variant-caps / text-transform) keeps a locale's own
    // tonos marks only when lang matches; a stale static lang="en" uppercases
    // Greek text as if it were English, dropping the tonos.
    vi.mocked(api.getLocale).mockResolvedValue('el')
    vi.mocked(api.setLocale).mockResolvedValue(undefined)
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(document.documentElement.lang).toBe('el')
    })

    setLocale('fr')
    await waitFor(() => {
      expect(document.documentElement.lang).toBe('fr')
    })
  })

  test('a failed change whose re-read lands after a newer saved change does not undo it', async () => {
    let finishReRead: (stored: string) => void = () => undefined
    const reRead = new Promise<string>((resolve) => {
      finishReRead = resolve
    })

    vi.mocked(api.getLocale).mockResolvedValueOnce('en').mockReturnValueOnce(reRead)
    vi.mocked(api.setLocale).mockRejectedValueOnce(new Error('disk full'))
    vi.mocked(api.setLocale).mockResolvedValueOnce(undefined)

    function FailureProbe() {
      const { locale, languageChangeFailed } = useI18n()
      return (
        <div>
          <span>{`locale:${locale}`}</span>
          <span>{`failed:${String(languageChangeFailed)}`}</span>
        </div>
      )
    }

    render(
      <I18nProvider>
        <FailureProbe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(api.getLocale).toHaveBeenCalledTimes(1)
    })

    setLocale('el')
    await waitFor(() => {
      expect(api.getLocale).toHaveBeenCalledTimes(2)
    })

    setLocale('fr')
    await waitFor(() => {
      expect(api.setLocale).toHaveBeenCalledWith('fr')
    })

    finishReRead('en')
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(getLocale()).toBe('fr')
    expect(screen.getByText('locale:fr')).toBeTruthy()
    expect(screen.getByText('failed:false')).toBeTruthy()
  })
})
