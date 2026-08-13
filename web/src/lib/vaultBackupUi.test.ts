import { describe, expect, test } from 'vitest'
import {
  backupCommandError,
  canBackupVault,
  restoreConfirm,
  vaultBackupAvailability,
  vaultBackupBanner,
} from './vaultBackupUi'

describe('vaultBackupAvailability', () => {
  test('ready when the vault has at least one entity', () => {
    expect(vaultBackupAvailability({ vaultPresent: true, entityCount: 1 })).toBe('ready')
  })

  test('empty when unlocked with zero entities', () => {
    expect(vaultBackupAvailability({ vaultPresent: true, entityCount: 0 })).toBe('empty')
  })

  test('missing when no vault files exist', () => {
    expect(vaultBackupAvailability({ vaultPresent: false, entityCount: 0 })).toBe('missing')
  })
})

describe('canBackupVault', () => {
  test('backup is disabled for empty and missing, enabled when ready', () => {
    expect(canBackupVault('ready')).toBe(true)
    expect(canBackupVault('empty')).toBe(false)
    expect(canBackupVault('missing')).toBe(false)
  })
})

describe('vaultBackupBanner', () => {
  test('empty copy matches backup-03', () => {
    expect(vaultBackupBanner('empty')).toEqual({
      title: 'Nothing to back up',
      body: 'This vault has not been initialized yet. Create a book first, or restore an existing backup.',
    })
  })

  test('missing copy matches backup-04', () => {
    expect(vaultBackupBanner('missing')).toEqual({
      title: 'No vault on this computer',
      body: 'Nothing to back up. Restore from a backup, or set up a new vault.',
    })
  })

  test('ready has no banner', () => {
    expect(vaultBackupBanner('ready')).toBeNull()
  })
})

describe('restoreConfirm', () => {
  test('uninitialized uses Load backup and replace:false', () => {
    const prompt = restoreConfirm('load')
    expect(prompt.title).toBe('Load backup on this device?')
    expect(prompt.confirmLabel).toBe('Load backup')
    expect(prompt.replace).toBe(false)
  })

  test('existing vault uses Replace vault and replace:true', () => {
    const prompt = restoreConfirm('replace')
    expect(prompt.title).toBe('Replace local vault?')
    expect(prompt.confirmLabel).toBe('Replace vault')
    expect(prompt.replace).toBe(true)
  })
})

describe('backupCommandError', () => {
  test('maps known backup/restore codes', () => {
    expect(backupCommandError({ code: 'vault_uninitialized', message: 'x' })).toMatch(
      /not been initialized/i,
    )
    expect(backupCommandError({ code: 'backup_invalid', message: 'backup is invalid: truncated' })).toBe(
      'backup is invalid: truncated',
    )
    expect(backupCommandError({ code: 'restore_would_overwrite', message: 'x' })).toMatch(/already exists/i)
    expect(backupCommandError({ code: 'not_found', message: 'file not found' })).toBe('file not found')
    expect(backupCommandError({ code: 'io', message: 'I/O error: disk' })).toBe('I/O error: disk')
  })
})
