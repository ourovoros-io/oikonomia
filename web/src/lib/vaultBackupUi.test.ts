import { afterEach, describe, expect, test, vi } from 'vitest'
import {
  backupCommandError,
  restoreCommandError,
  canBackupVault,
  restoreConfirm,
  vaultBackupAvailability,
  vaultBackupBanner,
} from './vaultBackupUi'
import { resetI18nForTests, t } from './i18n'
import en from '../locales/en.json' with { type: 'json' }

afterEach(() => {
  vi.restoreAllMocks()
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
      'That file is not a valid Oikonomia backup.',
    )
    expect(backupCommandError({ code: 'restore_would_overwrite', message: 'x' })).toMatch(/already exists/i)
    expect(backupCommandError({ code: 'not_found', message: 'file not found' })).toBe(
      'Backup file not found.',
    )
    expect(backupCommandError({ code: 'io', message: 'I/O error: disk' })).toBe(
      'Could not read or write the backup file.',
    )
  })

  test('logs the raw cause of a file error, which the sentence hides', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    backupCommandError({ code: 'io', message: 'EACCES /Users/x/backup' })

    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn.mock.calls[0][0]).toContain('EACCES /Users/x/backup')
  })

  test('a code with its own copy shows that copy, with its parameters', () => {
    expect(
      backupCommandError({
        code: 'file_too_large',
        message: 'file too large (max 8 MB)',
        params: { max_mb: '8' },
      }),
    ).toBe('That file is too large. The limit is 8 MB.')
  })

  test('an unknown code shows the backup fallback, never the raw message', () => {
    const shown = backupCommandError({ code: 'brand_new', message: 'sqlcipher: disk image is malformed' })

    expect(shown).toBe('Could not complete the backup.')
    expect(shown).not.toContain('sqlcipher')
  })
})

describe('restoreCommandError', () => {
  test('a vague code gets the restore sentence, where backup gets its own', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const error = { code: 'crypto', message: 'bad tag' }

    expect(restoreCommandError(error)).toBe('Could not restore the backup.')
    expect(backupCommandError(error)).toBe('Could not complete the backup.')
  })

  test('shares the specific sentences with backup', () => {
    for (const code of ['vault_uninitialized', 'backup_invalid', 'restore_would_overwrite', 'not_found']) {
      expect(restoreCommandError({ code, message: 'x' }), code).toBe(
        backupCommandError({ code, message: 'x' }),
      )
    }
  })
})
