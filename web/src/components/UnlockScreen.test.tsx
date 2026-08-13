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
}))

import { vaultRestore, vaultStatus } from '../lib/tauri'
import { UnlockScreen } from './UnlockScreen'

afterEach(() => {
  cleanup()
})

beforeEach(() => {
  vi.mocked(vaultRestore).mockReset()
  vi.mocked(vaultStatus).mockReset()
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

  test('uninitialized confirm uses Load backup then vaultRestore({ replace: false })', async () => {
    vi.mocked(vaultRestore).mockResolvedValue('/tmp/in.oikonomia-backup')
    vi.mocked(vaultStatus).mockResolvedValue('locked')
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="uninitialized" onUnlocked={onUnlocked} />)

    await userEvent.click(screen.getByRole('button', { name: 'Restore from backup' }))
    expect(vaultRestore).not.toHaveBeenCalled()
    expect(screen.getByRole('dialog', { name: 'Load backup on this device?' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Load backup' })).toBeTruthy()

    await userEvent.click(screen.getByRole('button', { name: 'Load backup' }))
    await waitFor(() => {
      expect(vaultRestore).toHaveBeenCalledWith({ replace: false })
    })
    expect(vi.mocked(vaultRestore).mock.calls[0]?.[0]).not.toHaveProperty('path')
    expect(vaultStatus).toHaveBeenCalledTimes(1)
    expect(onUnlocked).toHaveBeenCalledWith('locked')
  })

  test('locked confirm uses Replace vault then vaultRestore({ replace: true })', async () => {
    vi.mocked(vaultRestore).mockResolvedValue('/tmp/in.oikonomia-backup')
    vi.mocked(vaultStatus).mockResolvedValue('locked')
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="locked" onUnlocked={onUnlocked} />)

    await userEvent.click(screen.getByRole('button', { name: 'Restore from backup' }))
    expect(vaultRestore).not.toHaveBeenCalled()
    expect(screen.getByRole('dialog', { name: 'Replace local vault?' })).toBeTruthy()

    await userEvent.click(screen.getByRole('button', { name: 'Replace vault' }))
    await waitFor(() => {
      expect(vaultRestore).toHaveBeenCalledWith({ replace: true })
    })
    expect(onUnlocked).toHaveBeenCalledWith('locked')
  })

  test('cancelled restore (null) does not change status', async () => {
    vi.mocked(vaultRestore).mockResolvedValue(null)
    const onUnlocked = vi.fn()
    render(<UnlockScreen status="uninitialized" onUnlocked={onUnlocked} />)

    await userEvent.click(screen.getByRole('button', { name: 'Restore from backup' }))
    await userEvent.click(screen.getByRole('button', { name: 'Load backup' }))
    await waitFor(() => {
      expect(vaultRestore).toHaveBeenCalledWith({ replace: false })
    })
    expect(vaultStatus).not.toHaveBeenCalled()
    expect(onUnlocked).not.toHaveBeenCalled()
  })
})
