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
  updateCancel: vi.fn(),
  updateTakeNotice: vi.fn(async () => null),
}))

import { vaultPickBackup, vaultUnlock } from '../lib/tauri'
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

  test.each(['uninitialized', 'locked'] as const)(
    'in the %s state the password field has focus and the switch follows the form',
    (status) => {
      const { container } = render(<UnlockScreen status={status} onUnlocked={() => {}} />)

      expect(screen.getByLabelText(/^Password/)).toHaveFocus()

      const submit = container.querySelector('button[type="submit"]')
      const group = screen.getByRole('radiogroup', { name: 'Language' })
      expect(submit).not.toBeNull()
      const position = submit?.compareDocumentPosition(group) ?? 0
      expect(position & Node.DOCUMENT_POSITION_FOLLOWING).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
    },
  )
})

describe('UnlockScreen language switch focus', () => {
  test('the pill keeps focus after the language changes', async () => {
    setLocalePersist(vi.fn())
    const user = userEvent.setup()
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await user.click(screen.getByRole('radio', { name: 'Ελληνικά' }))

    expect(screen.getByRole('radio', { name: 'Ελληνικά' })).toHaveFocus()
  })
})

describe('UnlockScreen error language', () => {
  test('a wrong-password error is re-worded when the language changes', async () => {
    setLocalePersist(vi.fn())
    vi.mocked(vaultUnlock).mockRejectedValue({ code: 'invalid_password', message: '' })
    const user = userEvent.setup()
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await user.type(screen.getByLabelText('Password'), 'wrong')
    await user.click(screen.getByRole('button', { name: 'Unlock' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Incorrect password')

    await user.click(screen.getByRole('radio', { name: 'Ελληνικά' }))

    expect(screen.getByRole('alert')).toHaveTextContent('Λάθος κωδικός — δοκιμάστε ξανά.')
  })

  test('a restore error is re-worded when the language changes', async () => {
    setLocalePersist(vi.fn())
    vi.mocked(vaultPickBackup).mockRejectedValue({ code: 'backup_invalid', message: '' })
    const user = userEvent.setup()
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await user.click(screen.getByRole('button', { name: 'Restore from backup' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('not a valid')

    await user.click(screen.getByRole('radio', { name: 'Ελληνικά' }))

    expect(screen.getByRole('alert')).toHaveTextContent(
      'Αυτό το αρχείο δεν είναι έγκυρο αντίγραφο ασφαλείας Oikonomia.',
    )
  })
})
