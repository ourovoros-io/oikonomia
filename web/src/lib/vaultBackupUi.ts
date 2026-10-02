import type { CommandError } from './tauri'
import { commandErrorMessage } from './commandError'
import { t } from './i18n'

export type VaultBackupAvailability = 'ready' | 'empty' | 'missing'

export function vaultBackupBody(): string {
  return t('settings.vaultBackup.body')
}

export function vaultBackupHint(): string {
  return t('settings.vaultBackup.hint')
}

export function vaultBackupAvailability(opts: {
  vaultPresent: boolean
  entityCount: number
}): VaultBackupAvailability {
  if (!opts.vaultPresent) return 'missing'
  if (opts.entityCount === 0) return 'empty'
  return 'ready'
}

export function canBackupVault(availability: VaultBackupAvailability): boolean {
  return availability === 'ready'
}

export function vaultBackupBanner(availability: VaultBackupAvailability): {
  title: string
  body: string
} | null {
  switch (availability) {
    case 'empty':
      return {
        title: t('settings.vaultBackup.emptyTitle'),
        body: t('settings.vaultBackup.emptyBody'),
      }
    case 'missing':
      return {
        title: t('settings.vaultBackup.missingTitle'),
        body: t('settings.vaultBackup.missingBody'),
      }
    case 'ready':
      return null
  }
}

export type RestoreConfirmKind = 'load' | 'replace'

export function restoreConfirm(kind: RestoreConfirmKind): {
  title: string
  body: string
  confirmLabel: string
  replace: boolean
} {
  if (kind === 'load') {
    return {
      title: t('settings.restore.loadTitle'),
      body: t('settings.restore.loadBody'),
      confirmLabel: t('settings.restore.loadConfirm'),
      replace: false,
    }
  }
  return {
    title: t('settings.restore.replaceTitle'),
    body: t('settings.restore.replaceBody'),
    confirmLabel: t('settings.restore.replaceConfirm'),
    replace: true,
  }
}

/**
 * Backup and restore failures. A code with its own copy shows that copy; the
 * generic ones (invalid file, missing file, io) get a sentence about backups
 * that says what was being attempted.
 */
export function backupCommandError(err: CommandError): string {
  switch (err.code) {
    case 'vault_uninitialized':
      return t('settings.vaultBackup.errUninitialized')
    case 'restore_would_overwrite':
      return t('settings.vaultBackup.errOverwrite')
    case 'backup_invalid':
      return t('settings.vaultBackup.errInvalid')
    case 'not_found':
      return t('settings.vaultBackup.errNotFound')
    case 'io':
      return t('settings.vaultBackup.errIo')
    default:
      return commandErrorMessage(err, 'settings.vaultBackup.errDefault')
  }
}
