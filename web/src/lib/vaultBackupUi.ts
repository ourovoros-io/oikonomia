import { asCommandError } from './commandError'
import { t, type MessageKey } from './i18n'
import { commandFailure, copyError, type UiError } from './uiError'

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
  titleKey: MessageKey
  bodyKey: MessageKey
} | null {
  switch (availability) {
    case 'empty':
      return {
        titleKey: 'settings.vaultBackup.emptyTitle',
        bodyKey: 'settings.vaultBackup.emptyBody',
      }
    case 'missing':
      return {
        titleKey: 'settings.vaultBackup.missingTitle',
        bodyKey: 'settings.vaultBackup.missingBody',
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
 * Failures of backing up or restoring. A code with its own copy shows that
 * copy; the generic ones (invalid file, missing file, a file or database
 * failure) get a sentence about backups. `fallbackKey` says what was being
 * attempted for any other code, so a vague failure while restoring does not
 * read as one about making a backup.
 * Takes `unknown` because a rejection can be anything, including nothing.
 */
function vaultFailure(err: unknown, fallbackKey: MessageKey): UiError {
  switch (asCommandError(err).code) {
    case 'vault_uninitialized':
      return copyError('settings.vaultBackup.errUninitialized')
    case 'restore_would_overwrite':
      return copyError('settings.vaultBackup.errOverwrite')
    case 'backup_invalid':
      return copyError('settings.vaultBackup.errInvalid')
    case 'not_found':
      return copyError('settings.vaultBackup.errNotFound')
    case 'io':
    case 'database':
    case 'serialization':
      // The shared rule keeps this sentence over the generic file one and logs the cause.
      return commandFailure(err, 'settings.vaultBackup.errIo')
    default:
      return commandFailure(err, fallbackKey)
  }
}

/** The error for a failure while making a backup. */
export function backupFailure(err: unknown): UiError {
  return vaultFailure(err, 'settings.vaultBackup.errDefault')
}

/** The error for a failure while restoring a backup, or choosing the file to restore. */
export function restoreFailure(err: unknown): UiError {
  return vaultFailure(err, 'settings.vaultBackup.errRestoreDefault')
}
