/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { Entity } from '../lib/api'

vi.mock('../lib/tauri', () => ({
  vaultBackup: vi.fn(),
  vaultRestore: vi.fn(),
  vaultChangePassword: vi.fn(),
  pickVaultBackup: vi.fn(),
}))

vi.mock('../lib/api', () => ({
  api: {
    getLockTimeout: vi.fn(async () => 900),
  },
}))

import { pickVaultBackup, vaultBackup, vaultRestore } from '../lib/tauri'
import { SettingsPage } from './SettingsPage'

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

const BACKUP_PATH = '/tmp/backup.oikonomia-backup'

const noopAsync = async () => {}

afterEach(() => {
  cleanup()
})

beforeEach(() => {
  vi.mocked(vaultBackup).mockReset()
  vi.mocked(vaultRestore).mockReset()
  vi.mocked(pickVaultBackup).mockReset()
  vi.mocked(pickVaultBackup).mockResolvedValue({ kind: 'picked', path: BACKUP_PATH })
})

async function expandVaultBackup() {
  await userEvent.click(screen.getByRole('button', { name: /vault backup/i }))
}

describe('SettingsPage vault backup', () => {
  test('disables Backup vault when there are no entities', async () => {
    render(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    expect(screen.getByRole('button', { name: /backup vault/i })).toBeDisabled()
    expect(screen.getByRole('button', { name: /restore from backup/i })).toBeEnabled()
    expect(screen.getByText('Nothing to back up')).toBeTruthy()
    expect(
      screen.getByText(
        'This vault has not been initialized yet. Create a book first, or restore an existing backup.',
      ),
    ).toBeTruthy()
  })

  test('disables Backup vault when the vault is missing', async () => {
    render(
      <SettingsPage
        entities={[]}
        vaultPresent={false}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    expect(screen.getByRole('button', { name: /backup vault/i })).toBeDisabled()
    expect(screen.getByRole('button', { name: /restore from backup/i })).toBeEnabled()
    expect(screen.getByText('No vault on this computer')).toBeTruthy()
    expect(
      screen.getByText('Nothing to back up. Restore from a backup, or set up a new vault.'),
    ).toBeTruthy()
  })

  test('enables Backup vault when entities exist', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    expect(screen.getByRole('button', { name: /backup vault/i })).toBeEnabled()
  })

  test('opens the picker first, then ConfirmDialog, then vaultRestore with path', async () => {
    vi.mocked(vaultRestore).mockResolvedValue(BACKUP_PATH)
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    await userEvent.click(screen.getByRole('button', { name: /restore from backup/i }))
    await waitFor(() => {
      expect(pickVaultBackup).toHaveBeenCalledTimes(1)
    })
    expect(vaultRestore).not.toHaveBeenCalled()
    expect(screen.getByRole('dialog', { name: 'Replace local vault?' })).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Replace vault' }))
    await waitFor(() => {
      expect(vaultRestore).toHaveBeenCalledTimes(1)
    })
    expect(vaultRestore).toHaveBeenCalledWith({ path: BACKUP_PATH, replace: true })
  })

  test('cancelled picker does not open ConfirmDialog or call vaultRestore', async () => {
    vi.mocked(pickVaultBackup).mockResolvedValue({ kind: 'cancelled' })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    await userEvent.click(screen.getByRole('button', { name: /restore from backup/i }))
    await waitFor(() => {
      expect(pickVaultBackup).toHaveBeenCalledTimes(1)
    })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(vaultRestore).not.toHaveBeenCalled()
  })
})
