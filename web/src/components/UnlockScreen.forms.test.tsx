/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

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

import { vaultInit, vaultPickBackup, vaultUnlock } from '../lib/tauri'
import { UnlockScreen } from './UnlockScreen'
import { resetI18nForTests } from '../lib/i18n'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(vaultInit).mockReset().mockResolvedValue('unlocked')
  vi.mocked(vaultUnlock).mockReset().mockResolvedValue('unlocked')
  vi.mocked(vaultPickBackup).mockReset().mockResolvedValue('/tmp/in.oikonomia-backup')
})

describe('UnlockScreen form errors', () => {
  test('an empty password is reported in the app own words, with no browser bubble', async () => {
    const { container } = render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await userEvent.click(screen.getByRole('button', { name: 'Unlock' }))

    expect(container.querySelector('form')).toHaveAttribute('novalidate')
    expect(screen.getByRole('alert')).toHaveTextContent('Enter your password.')
    expect(vaultUnlock).not.toHaveBeenCalled()
    expect(screen.getByLabelText('Password')).toHaveFocus()
  })

  test('a refused short password keeps what was typed and returns to the password field', async () => {
    vi.mocked(vaultInit).mockRejectedValue({
      code: 'password_too_short',
      message: 'too short',
      params: { min: '12' },
    })
    render(<UnlockScreen status="uninitialized" onUnlocked={() => {}} />)
    await userEvent.type(screen.getByLabelText('Password'), 'short-pass')
    await userEvent.type(screen.getByLabelText('Confirm password'), 'short-pass')

    await userEvent.click(screen.getByRole('button', { name: 'Create encrypted vault' }))

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('too short')
    })
    expect(screen.getByLabelText('Password')).toHaveValue('short-pass')
    expect(screen.getByLabelText('Confirm password')).toHaveValue('short-pass')
    await waitFor(() => expect(screen.getByLabelText('Password')).toHaveFocus())
  })

  test('mismatched passwords are kept too, and the caret goes to Confirm', async () => {
    render(<UnlockScreen status="uninitialized" onUnlocked={() => {}} />)
    await userEvent.type(screen.getByLabelText('Password'), 'alpha-alpha')
    await userEvent.type(screen.getByLabelText('Confirm password'), 'alpha-beta')

    await userEvent.click(screen.getByRole('button', { name: 'Create encrypted vault' }))

    expect(screen.getByLabelText('Password')).toHaveValue('alpha-alpha')
    expect(screen.getByLabelText('Confirm password')).toHaveValue('alpha-beta')
    await waitFor(() => expect(screen.getByLabelText('Confirm password')).toHaveFocus())
  })

  test('a wrong unlock password is cleared and the field is ready for the next try', async () => {
    vi.mocked(vaultUnlock).mockRejectedValue({ code: 'invalid_password', message: '' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await userEvent.type(screen.getByLabelText('Password'), 'wrong')

    await userEvent.click(screen.getByRole('button', { name: 'Unlock' }))

    await waitFor(() => expect(screen.getByLabelText('Password')).toHaveValue(''))
    await waitFor(() => expect(screen.getByLabelText('Password')).toHaveFocus())
  })

  test('closing the restore dialog puts the caret back in the password field', async () => {
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await userEvent.click(screen.getByRole('button', { name: 'Restore from backup' }))
    await userEvent.click(await screen.findByRole('button', { name: 'Cancel' }))

    await waitFor(() => expect(screen.getByLabelText('Password')).toHaveFocus())
  })
})
