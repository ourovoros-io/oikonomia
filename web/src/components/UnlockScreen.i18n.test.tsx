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
import { resetI18nForTests, setLocale } from '../lib/i18n'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

describe('UnlockScreen i18n', () => {
  test('setup title uses Writer catalog', () => {
    setLocale('el')
    render(<UnlockScreen status="uninitialized" onUnlocked={() => {}} />)
    expect(screen.getByRole('heading', { name: 'Δημιουργία θυρίδας' })).toBeTruthy()
    expect(
      screen.getByText(
        'Επιλέξτε κωδικό. Δεν αποθηκεύεται. Αν τον χάσετε, τα βιβλία δεν ανακτώνται.',
      ),
    ).toBeTruthy()
  })

  test('unlock title uses Writer catalog', () => {
    setLocale('el')
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    expect(screen.getByRole('heading', { name: 'Καλωσορίσατε' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Ξεκλείδωμα' })).toBeTruthy()
  })
})
