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
    getLocale: vi.fn(async () => 'en'),
    setLocale: vi.fn(async () => undefined),
    getUiPrefs: vi.fn(async () => ({ locale: 'en' })),
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
  vi.mocked(api.getLocale).mockReset()
  vi.mocked(api.getLocale).mockResolvedValue('en')
  vi.mocked(api.setLocale).mockReset()
  vi.mocked(api.setLocale).mockResolvedValue(undefined)
  vi.mocked(api.getUiPrefs).mockReset()
})

describe('SettingsPage i18n', () => {
  test('heading and language chrome render Writer el catalog', async () => {
    setLocale('el')
    render(
      <SettingsPage entities={[]} onEntitiesChange={async () => {}} onSelectEntity={() => {}} />,
    )
    expect(screen.getByRole('heading', { name: 'Ρυθμίσεις' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Γλώσσα' })).toBeTruthy()
    expect(screen.getByText('Μενού, ετικέτες και Γρήγορη καταχώριση.')).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: /γλώσσα/i }))
    expect(screen.getByRole('radio', { name: 'English' })).toBeTruthy()
    expect(screen.getByRole('radio', { name: 'Ελληνικά' })).toBeTruthy()
    expect(screen.getByRole('radio', { name: 'Français' })).toBeTruthy()
    expect(screen.getByRole('radio', { name: 'Deutsch' })).toBeTruthy()
    expect(screen.getAllByRole('radio')).toHaveLength(4)
    const group = screen.getByRole('radiogroup', { name: 'Γλώσσα' })
    expect(group.className).toMatch(/\bgrid\b/)
    expect(group.className).toMatch(/\bgrid-flow-col\b/)
    expect(group.className).toMatch(/\bw-fit\b/)
    expect(group.className).not.toMatch(/\bgrid-cols-2\b/)
    expect(group.className).not.toMatch(/\bflex-wrap\b/)
    for (const radio of screen.getAllByRole('radio')) {
      expect(radio.className).toMatch(/\bw-full\b/)
      expect(radio.className).toMatch(/\bjustify-center\b/)
    }
  })

  test('language pill persists via settings_set_locale and mirrors oikonomia.locale', async () => {
    render(
      <I18nProvider>
        <SettingsPage entities={[]} onEntitiesChange={async () => {}} onSelectEntity={() => {}} />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(api.getLocale).toHaveBeenCalled()
    })
    await userEvent.click(screen.getByRole('button', { name: /language/i }))
    const choices = [
      { locale: 'el' as const, label: 'Ελληνικά' },
      { locale: 'fr' as const, label: 'Français' },
      { locale: 'de' as const, label: 'Deutsch' },
      { locale: 'en' as const, label: 'English' },
    ]
    for (const choice of choices) {
      await userEvent.click(screen.getByRole('radio', { name: choice.label }))
      await waitFor(() => {
        expect(getLocale()).toBe(choice.locale)
        expect(api.setLocale).toHaveBeenCalledWith(choice.locale)
        expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe(choice.locale)
      })
    }
  })
})
