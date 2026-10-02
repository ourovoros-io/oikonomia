/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/tauri', () => ({
  vaultInit: vi.fn(),
  vaultUnlock: vi.fn(),
  vaultRestore: vi.fn(),
  vaultStatus: vi.fn(),
  vaultPickBackup: vi.fn(),
  updateCheck: vi.fn(),
  updateInstall: vi.fn(),
}))

import { UnlockScreen } from './UnlockScreen'
import { resetI18nForTests, setLocale, setLocalePersist } from '../lib/i18n'

afterEach(() => {
  cleanup()
  setLocalePersist(null)
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
    expect(screen.getByRole('button', { name: 'Έλεγχος ενημέρωσης' })).toBeTruthy()
  })
})

describe('UnlockScreen language control', () => {
  test('the create-vault state offers the language switch', async () => {
    const persist = vi.fn()
    setLocalePersist(persist)
    const user = userEvent.setup()
    render(<UnlockScreen status="uninitialized" onUnlocked={() => {}} />)

    const group = screen.getByRole('radiogroup', { name: 'Language' })
    expect(group).toBeTruthy()
    expect(screen.getByRole('radio', { name: 'English' })).toBeChecked()

    await user.click(screen.getByRole('radio', { name: 'Ελληνικά' }))

    expect(screen.getByRole('heading', { name: 'Δημιουργία θυρίδας' })).toBeTruthy()
    expect(persist).toHaveBeenCalledWith('el')
  })

  test('the unlock state offers it too', async () => {
    const persist = vi.fn()
    setLocalePersist(persist)
    const user = userEvent.setup()
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await user.click(screen.getByRole('radio', { name: 'Deutsch' }))

    expect(screen.getByRole('heading', { name: 'Willkommen zurück' })).toBeTruthy()
    expect(persist).toHaveBeenCalledWith('de')
  })

  test('is reachable with the keyboard', async () => {
    const user = userEvent.setup()
    render(<UnlockScreen status="uninitialized" onUnlocked={() => {}} />)

    const french = screen.getByRole('radio', { name: 'Français' })
    for (let presses = 0; presses < 12 && document.activeElement !== french; presses += 1) {
      await user.tab()
    }

    expect(french).toHaveFocus()
  })
})
