import { afterEach, describe, expect, test } from 'vitest'
import {
  backupCommandError,
  canBackupVault,
  restoreConfirm,
  vaultBackupAvailability,
  vaultBackupBanner,
} from './vaultBackupUi'
import { resetI18nForTests, t } from './i18n'
import en from '../locales/en.json' with { type: 'json' }

afterEach(() => {
  resetI18nForTests()
})

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
      title: t('settings.vaultBackup.emptyTitle'),
      body: t('settings.vaultBackup.emptyBody'),
    })
    expect(vaultBackupBanner('empty')?.title).toBe(en['settings.vaultBackup.emptyTitle'])
  })

  test('missing copy matches backup-04', () => {
    expect(vaultBackupBanner('missing')).toEqual({
      title: t('settings.vaultBackup.missingTitle'),
      body: t('settings.vaultBackup.missingBody'),
    })
    expect(vaultBackupBanner('missing')?.title).toBe(en['settings.vaultBackup.missingTitle'])
  })

  test('ready has no banner', () => {
    expect(vaultBackupBanner('ready')).toBeNull()
  })
})

describe('restoreConfirm', () => {
  test('uninitialized uses Load backup and replace:false', () => {
    const prompt = restoreConfirm('load')
    expect(prompt.title).toBe(t('settings.restore.loadTitle'))
    expect(prompt.confirmLabel).toBe(t('settings.restore.loadConfirm'))
    expect(prompt.replace).toBe(false)
  })

  test('existing vault uses Replace vault and replace:true', () => {
    const prompt = restoreConfirm('replace')
    expect(prompt.title).toBe(t('settings.restore.replaceTitle'))
    expect(prompt.confirmLabel).toBe(t('settings.restore.replaceConfirm'))
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
