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

import { vaultPickBackup, vaultRestore, vaultStatus } from '../lib/tauri'
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
