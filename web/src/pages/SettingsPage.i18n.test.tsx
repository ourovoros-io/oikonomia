/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/tauri', () => ({
  vaultBackup: vi.fn(),
  vaultRestore: vi.fn(),
  vaultPickBackup: vi.fn(),
  vaultChangePassword: vi.fn(),
}))

vi.mock('../lib/api', () => ({
  api: {
    getLockTimeout: vi.fn(async () => 900),
    setLocale: vi.fn(async () => undefined),
  },
}))

import { api } from '../lib/api'
import { I18nProvider } from '../lib/I18nProvider'
import { SettingsPage } from './SettingsPage'
import { LOCALE_STORAGE_KEY, getLocale, resetI18nForTests, setLocale } from '../lib/i18n'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(api.getLockTimeout).mockReset()
  vi.mocked(api.getLockTimeout).mockResolvedValue(900)
  vi.mocked(api.setLocale).mockReset()
})

describe('SettingsPage i18n', () => {
  test('heading and language chrome render Writer el catalog', () => {
    setLocale('el')
    render(
      <SettingsPage entities={[]} onEntitiesChange={async () => {}} onSelectEntity={() => {}} />,
    )
    expect(screen.getByRole('heading', { name: 'Ρυθμίσεις' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Γλώσσα' })).toBeTruthy()
    expect(screen.getByText('Μενού, ετικέτες και Γρήγορη καταχώριση.')).toBeTruthy()
    expect(screen.getByRole('radio', { name: 'English' })).toBeTruthy()
    expect(screen.getByRole('radio', { name: 'Ελληνικά' })).toBeTruthy()
  })

  test('language pill persists oikonomia.locale and does not call settings_set_locale', async () => {
    render(
      <I18nProvider>
        <SettingsPage entities={[]} onEntitiesChange={async () => {}} onSelectEntity={() => {}} />
      </I18nProvider>,
    )
    await userEvent.click(screen.getByRole('radio', { name: 'Ελληνικά' }))
    await waitFor(() => {
      expect(getLocale()).toBe('el')
      expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe('el')
    })
    expect(api.setLocale).not.toHaveBeenCalled()
    expect(screen.getByRole('heading', { name: 'Ρυθμίσεις' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Γλώσσα' })).toBeTruthy()
  })
})
