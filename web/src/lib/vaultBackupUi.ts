import type { CommandError } from './tauri'

export type VaultBackupAvailability = 'ready' | 'empty' | 'missing'

export const VAULT_BACKUP_BODY =
  'Files stay ciphertext. Backup packs the vault header and database together so they stay a pair. Unlock still uses your master password.'

export const VAULT_BACKUP_HINT = 'Saved as oikonomia-backup-YYYY-MM-DD.oikonomia-backup'

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
        title: 'Nothing to back up',
        body: 'This vault has not been initialized yet. Create a book first, or restore an existing backup.',
      }
    case 'missing':
      return {
        title: 'No vault on this computer',
        body: 'Nothing to back up. Restore from a backup, or set up a new vault.',
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
      title: 'Load backup on this device?',
      body: 'This becomes the vault on this computer. After it loads, the vault will be locked — unlock with your existing master password.',
      confirmLabel: 'Load backup',
      replace: false,
    }
  }
  return {
    title: 'Replace local vault?',
    body: 'Restoring this backup replaces the vault on this computer. The current vault cannot be recovered. After restore, the vault will be locked — unlock with your master password.',
    confirmLabel: 'Replace vault',
    replace: true,
  }
}

export function restoreArgs(
  path: string | undefined,
  replace: boolean,
): { path?: string; replace: boolean } {
  return path === undefined ? { replace } : { path, replace }
}

export function backupCommandError(err: CommandError): string {
  switch (err.code) {
    case 'vault_uninitialized':
      return 'Nothing to back up. This vault has not been initialized yet.'
    case 'backup_invalid':
      return err.message || 'That file is not a valid Oikonomia backup.'
    case 'restore_would_overwrite':
      return 'A vault already exists on this computer. Confirm replace to continue.'
    case 'not_found':
      return err.message || 'Backup file not found.'
    case 'io':
      return err.message || 'Could not read or write the backup file.'
    default:
      return err.message || 'Could not complete the backup.'
  }
}
