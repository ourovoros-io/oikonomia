/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/tauri', () => ({
  vaultInit: vi.fn(),
  vaultUnlock: vi.fn(),
  vaultRestore: vi.fn(),
  vaultStatus: vi.fn(),
  vaultPickBackup: vi.fn(),
}))

import { UnlockScreen } from './UnlockScreen'
import { resetI18nForTests, setLocale, setLocaleMessagesForTests } from '../lib/i18n'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

describe('UnlockScreen i18n', () => {
  test('renders create title via key; el stub is looked up', () => {
    setLocaleMessagesForTests('el', { 'unlock.titleCreate': 'EL Create your vault' })
    setLocale('el')
    render(<UnlockScreen status="uninitialized" onUnlocked={() => {}} />)
    expect(screen.getByRole('heading', { name: 'EL Create your vault' })).toBeTruthy()
  })
})
