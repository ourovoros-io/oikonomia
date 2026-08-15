/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/tauri', () => ({
  vaultBackup: vi.fn(),
  vaultRestore: vi.fn(),
  vaultPickBackup: vi.fn(),
  vaultChangePassword: vi.fn(),
}))

vi.mock('../lib/api', () => ({
  api: {
    getLockTimeout: vi.fn(async () => 900),
  },
}))

import { SettingsPage } from './SettingsPage'
import { resetI18nForTests, setLocale } from '../lib/i18n'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

describe('SettingsPage i18n', () => {
  test('heading renders Writer el catalog; language switcher is held', () => {
    setLocale('el')
    render(
      <SettingsPage entities={[]} onEntitiesChange={async () => {}} onSelectEntity={() => {}} />,
    )
    expect(screen.getByRole('heading', { name: 'Ρυθμίσεις' })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: 'Γλώσσα' })).toBeNull()
    expect(screen.queryByRole('radio', { name: 'Ελληνικά' })).toBeNull()
  })
})
