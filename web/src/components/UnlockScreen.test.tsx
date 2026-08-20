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
}))

import { vaultInit, vaultPickBackup, vaultRestore, vaultStatus, vaultUnlock } from '../lib/tauri'
import { UnlockScreen } from './UnlockScreen'
import { resetI18nForTests } from '../lib/i18n'

const BACKUP_PATH = '/tmp/in.oikonomia-backup'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(vaultRestore).mockReset()
  vi.mocked(vaultStatus).mockReset()
  vi.mocked(vaultPickBackup).mockReset()
  vi.mocked(vaultPickBackup).mockResolvedValue(BACKUP_PATH)
  vi.mocked(vaultInit).mockReset().mockResolvedValue('unlocked')
  vi.mocked(vaultUnlock).mockReset().mockResolvedValue('unlocked')
})

describe('UnlockScreen submit', () => {
  test('setup shows confirm field; mismatch uses unlock.passwordsMismatch', async () => {
    render(<UnlockScreen status="uninitialized" onUnlocked={() => {}} />)
    expect(screen.getByLabelText('Confirm password')).toBeTruthy()
    await userEvent.type(screen.getByLabelText('Password'), 'alpha')
    await userEvent.type(screen.getByLabelText('Confirm password'), 'beta')
    await userEvent.click(screen.getByRole('button', { name: 'Create encrypted vault' }))
    expect(screen.getByText('Passwords do not match')).toBeTruthy()
    expect(vaultInit).not.toHaveBeenCalled()
    expect(vaultUnlock).not.toHaveBeenCalled()
  })

  test('setup matching submit calls vaultInit, not vaultUnlock', async () => {
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="uninitialized" onUnlocked={onUnlocked} />)
    await userEvent.type(screen.getByLabelText('Password'), 'secret')
    await userEvent.type(screen.getByLabelText('Confirm password'), 'secret')
    await userEvent.click(screen.getByRole('button', { name: 'Create encrypted vault' }))
    await waitFor(() => {
      expect(vaultInit).toHaveBeenCalledWith('secret')
    })
    expect(vaultUnlock).not.toHaveBeenCalled()
    expect(onUnlocked).toHaveBeenCalledWith('unlocked')
  })

  test('locked has no confirm field and submit calls vaultUnlock', async () => {
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="locked" onUnlocked={onUnlocked} />)
    expect(screen.queryByLabelText('Confirm password')).toBeNull()
    await userEvent.type(screen.getByLabelText('Password'), 'secret')
    await userEvent.click(screen.getByRole('button', { name: 'Unlock' }))
    await waitFor(() => {
      expect(vaultUnlock).toHaveBeenCalledWith('secret')
    })
    expect(vaultInit).not.toHaveBeenCalled()
    expect(onUnlocked).toHaveBeenCalledWith('unlocked')
  })

  test('invalid_password maps to Incorrect-password banner, never the Rust message', async () => {
    vi.mocked(vaultUnlock).mockRejectedValue({
      code: 'invalid_password',
      message: 'sqlcipher: file is not a database',
    })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await userEvent.type(screen.getByLabelText('Password'), 'wrong')
    await userEvent.click(screen.getByRole('button', { name: 'Unlock' }))
    await waitFor(() => {
      expect(screen.getByText('Incorrect password — please try again.')).toBeTruthy()
    })
    expect(screen.queryByText('sqlcipher: file is not a database')).toBeNull()
  })

  test('generic unlock failure shows unlock.unlockFailed banner', async () => {
    vi.mocked(vaultUnlock).mockRejectedValue({ code: 'unknown', message: '' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await userEvent.type(screen.getByLabelText('Password'), 'secret')
    await userEvent.click(screen.getByRole('button', { name: 'Unlock' }))
    await waitFor(() => {
      expect(screen.getByText('Could not unlock the vault.')).toBeTruthy()
    })
  })
})

describe('UnlockScreen restore CommandError banners', () => {
  const cases: Array<{ code: string; text: string }> = [
    {
      code: 'vault_uninitialized',
      text: 'Nothing to back up. This vault has not been initialized yet.',
    },
    {
      code: 'backup_invalid',
      text: 'That file is not a valid Oikonomia backup.',
    },
    {
      code: 'restore_would_overwrite',
      text: 'A vault already exists on this computer. Confirm replace to continue.',
    },
    { code: 'not_found', text: 'Backup file not found.' },
    { code: 'io', text: 'Could not read or write the backup file.' },
  ]

  test.each(cases)('vaultRestore $code renders ErrorBanner', async ({ code, text }) => {
    vi.mocked(vaultRestore).mockRejectedValue({ code, message: '' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: 'Restore from backup' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Replace local vault?' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Replace vault' }))
    await waitFor(() => {
      expect(screen.getByText(text)).toBeTruthy()
    })
  })
})

describe('UnlockScreen restore whisper', () => {
  test('shows Restore from backup on uninitialized and locked', () => {
    const { rerender } = render(
      <UnlockScreen status="uninitialized" onUnlocked={() => {}} />,
    )
    expect(screen.getByRole('button', { name: 'Restore from backup' })).toBeTruthy()

    rerender(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    expect(screen.getByRole('button', { name: 'Restore from backup' })).toBeTruthy()
  })

  test('uninitialized: pick then Load backup then vaultRestore({ path, replace: false })', async () => {
    vi.mocked(vaultRestore).mockResolvedValue(BACKUP_PATH)
    vi.mocked(vaultStatus).mockResolvedValue('locked')
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="uninitialized" onUnlocked={onUnlocked} />)

    await userEvent.click(screen.getByRole('button', { name: 'Restore from backup' }))
    await waitFor(() => {
      expect(vaultPickBackup).toHaveBeenCalledTimes(1)
    })
    expect(vaultRestore).not.toHaveBeenCalled()
    expect(screen.getByRole('dialog', { name: 'Load backup on this device?' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Load backup' })).toBeTruthy()

    await userEvent.click(screen.getByRole('button', { name: 'Load backup' }))
    await waitFor(() => {
      expect(vaultRestore).toHaveBeenCalledWith({ path: BACKUP_PATH, replace: false })
    })
    expect(vaultStatus).toHaveBeenCalledTimes(1)
    expect(onUnlocked).toHaveBeenCalledWith('locked')
  })

  test('locked: pick then Replace vault then vaultRestore({ path, replace: true })', async () => {
    vi.mocked(vaultRestore).mockResolvedValue(BACKUP_PATH)
    vi.mocked(vaultStatus).mockResolvedValue('locked')
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="locked" onUnlocked={onUnlocked} />)

    await userEvent.click(screen.getByRole('button', { name: 'Restore from backup' }))
    await waitFor(() => {
      expect(vaultPickBackup).toHaveBeenCalledTimes(1)
    })
    expect(vaultRestore).not.toHaveBeenCalled()
    expect(screen.getByRole('dialog', { name: 'Replace local vault?' })).toBeTruthy()

    await userEvent.click(screen.getByRole('button', { name: 'Replace vault' }))
    await waitFor(() => {
      expect(vaultRestore).toHaveBeenCalledWith({ path: BACKUP_PATH, replace: true })
    })
    expect(onUnlocked).toHaveBeenCalledWith('locked')
  })

  test('cancelled pick does not restore and does not change status', async () => {
    vi.mocked(vaultPickBackup).mockResolvedValue(null)
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="uninitialized" onUnlocked={onUnlocked} />)

    await userEvent.click(screen.getByRole('button', { name: 'Restore from backup' }))
    await waitFor(() => {
      expect(vaultPickBackup).toHaveBeenCalledTimes(1)
    })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(vaultRestore).not.toHaveBeenCalled()
    expect(vaultStatus).not.toHaveBeenCalled()
    expect(onUnlocked).not.toHaveBeenCalled()
  })
})
