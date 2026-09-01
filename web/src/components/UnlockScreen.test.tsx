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
}))

import {
  updateCheck,
  updateInstall,
  vaultInit,
  vaultPickBackup,
  vaultRestore,
  vaultStatus,
  vaultUnlock,
} from '../lib/tauri'
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
  vi.mocked(updateCheck).mockReset().mockResolvedValue({ kind: 'upToDate' })
  vi.mocked(updateInstall).mockReset().mockResolvedValue(undefined)
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
    await waitFor(() => {
      expect(onUnlocked).toHaveBeenCalledWith('unlocked')
    })
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
    await waitFor(() => {
      expect(onUnlocked).toHaveBeenCalledWith('unlocked')
    })
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

describe('UnlockScreen success beat', () => {
  test('unlock handoff waits for the success beat before reporting status', async () => {
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="locked" onUnlocked={onUnlocked} />)
    await userEvent.type(screen.getByLabelText('Password'), 'secret')
    await userEvent.click(screen.getByRole('button', { name: 'Unlock' }))
    await waitFor(() => {
      expect(vaultUnlock).toHaveBeenCalledWith('secret')
    })

    // The beat plays first; the status handoff lands only after it.
    expect(onUnlocked).not.toHaveBeenCalled()
    await waitFor(() => {
      expect(onUnlocked).toHaveBeenCalledWith('unlocked')
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

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((res) => {
    resolve = res
  })
  return { promise, resolve }
}

describe('UnlockScreen check for update', () => {
  test('idle stacks Check for update above Restore; Unlock still submits', async () => {
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="locked" onUnlocked={onUnlocked} />)

    const check = screen.getByRole('button', { name: 'Check for update' })
    const restore = screen.getByRole('button', { name: 'Restore from backup' })
    expect(
      check.compareDocumentPosition(restore) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy()
    expect(screen.queryByRole('dialog')).toBeNull()

    await userEvent.type(screen.getByLabelText('Password'), 'secret')
    await userEvent.click(screen.getByRole('button', { name: 'Unlock' }))
    await waitFor(() => {
      expect(vaultUnlock).toHaveBeenCalledWith('secret')
    })
    await waitFor(() => {
      expect(onUnlocked).toHaveBeenCalledWith('unlocked')
    })
    expect(updateCheck).not.toHaveBeenCalled()
  })

  test('Check for update opens checking; password and Unlock stay enabled; foot label unchanged', async () => {
    const pending = deferred<{ kind: 'upToDate' }>()
    vi.mocked(updateCheck).mockReturnValue(pending.promise)

    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await userEvent.type(screen.getByLabelText('Password'), 'secret')
    await userEvent.click(screen.getByRole('button', { name: 'Check for update' }))

    expect(screen.getByRole('dialog', { name: 'Checking' })).toBeTruthy()
    expect(screen.getByText('Looking for a new application.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeTruthy()

    const password = screen.getByLabelText('Password')
    const unlock = screen.getByRole('button', { name: 'Unlock' })
    expect(password).not.toBeDisabled()
    expect(unlock).not.toBeDisabled()
    expect(unlock).toHaveTextContent('Unlock')
    expect(screen.getByRole('button', { name: 'Check for update' })).toHaveTextContent(
      'Check for update',
    )
    expect(screen.queryByText('What’s new')).toBeNull()
  })

  test('Cancel from checking closes the dialog and ignores a late result; Unlock still works', async () => {
    const pending = deferred<{ kind: 'available'; version: string }>()
    vi.mocked(updateCheck).mockReturnValue(pending.promise)
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="locked" onUnlocked={onUnlocked} />)

    await userEvent.click(screen.getByRole('button', { name: 'Check for update' }))
    expect(screen.getByRole('dialog', { name: 'Checking' })).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(screen.queryByRole('dialog')).toBeNull()

    pending.resolve({ kind: 'available', version: '9.9.9' })
    await waitFor(() => {
      expect(screen.queryByText('Oikonomia 9.9.9')).toBeNull()
    })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(updateInstall).not.toHaveBeenCalled()

    await userEvent.type(screen.getByLabelText('Password'), 'secret')
    await userEvent.click(screen.getByRole('button', { name: 'Unlock' }))
    await waitFor(() => {
      expect(vaultUnlock).toHaveBeenCalledWith('secret')
    })
    await waitFor(() => {
      expect(onUnlocked).toHaveBeenCalledWith('unlocked')
    })
  })

  test('stub upToDate shows Writer copy and Close', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'upToDate' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: 'Check for update' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'You’re up to date' })).toBeTruthy()
    })
    expect(screen.getByText('This is the latest Oikonomia.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Close' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Install and restart' })).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  test('available renders version + honesty, never notes or size, and install is the only invoke', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.1' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: 'Check for update' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'A new application is ready' })).toBeTruthy()
    })
    expect(screen.getByText('Oikonomia 0.1.1')).toBeTruthy()
    expect(
      screen.getByText(
        'This is the only internet contact, and only to fetch a new application.',
      ),
    ).toBeTruthy()
    expect(screen.queryByText('What’s new')).toBeNull()
    expect(screen.queryByText('{size}')).toBeNull()
    expect(screen.queryByText('12 MB')).toBeNull()
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeTruthy()

    await userEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    await waitFor(() => {
      expect(updateInstall).toHaveBeenCalledTimes(1)
    })
    expect(updateInstall).toHaveBeenCalledWith({ kind: 'available', version: '0.1.1' })
    expect(screen.getByRole('dialog', { name: 'Installing' })).toBeTruthy()
    expect(screen.getByText('Oikonomia will restart when this finishes.')).toBeTruthy()
    expect(screen.getByRole('progressbar')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Close' })).toBeNull()
  })

  test('install returning failed shows Failed and Unlock stays usable', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.1' })
    vi.mocked(updateInstall).mockResolvedValue({ kind: 'failed' })
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="locked" onUnlocked={onUnlocked} />)
    await userEvent.click(screen.getByRole('button', { name: 'Check for update' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Install and restart' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Couldn’t check' })).toBeTruthy()
    })
    expect(screen.getByLabelText('Password')).not.toBeDisabled()
    expect(screen.getByRole('button', { name: 'Unlock' })).not.toBeDisabled()
    await userEvent.type(screen.getByLabelText('Password'), 'secret')
    await userEvent.click(screen.getByRole('button', { name: 'Unlock' }))
    await waitFor(() => {
      expect(vaultUnlock).toHaveBeenCalledWith('secret')
    })
    await waitFor(() => {
      expect(onUnlocked).toHaveBeenCalledWith('unlocked')
    })
  })

  test('failed is Close only — no retry, no Settings', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'failed' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: 'Check for update' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Couldn’t check' })).toBeTruthy()
    })
    expect(screen.getByText('You stay on this version.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Close' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Settings' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Install and restart' })).toBeNull()
    expect(updateInstall).not.toHaveBeenCalled()
  })

  test('honesty is the Writer string, not a feed field', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '1.2.3' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: 'Check for update' }))
    await waitFor(() => {
      expect(screen.getByText('Oikonomia 1.2.3')).toBeTruthy()
    })
    expect(screen.queryByText('FEED HONESTY')).toBeNull()
    expect(
      screen.getByText(
        'This is the only internet contact, and only to fetch a new application.',
      ),
    ).toBeTruthy()
  })

  test('update_install is not invoked from upToDate, failed, or checking', async () => {
    const pending = deferred<{ kind: 'upToDate' }>()
    vi.mocked(updateCheck).mockReturnValueOnce(pending.promise)
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: 'Check for update' }))
    expect(screen.getByRole('dialog', { name: 'Checking' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Install and restart' })).toBeNull()
    expect(updateInstall).not.toHaveBeenCalled()
    pending.resolve({ kind: 'upToDate' })
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'You’re up to date' })).toBeTruthy()
    })
    expect(updateInstall).not.toHaveBeenCalled()
  })
})
