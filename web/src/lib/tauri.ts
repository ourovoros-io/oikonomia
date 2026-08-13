import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

export type VaultStatus = 'uninitialized' | 'locked' | 'unlocked'

export type AppInfo = {
  version: string
  name: string
}

export type CommandError = {
  code: string
  message: string
}

const BACKUP_FILTER = {
  name: 'Oikonomia backup',
  extensions: ['oikonomia-backup'],
} as const

/** True when running inside the Tauri webview (not a plain browser tab). */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

function asCommandError(err: unknown): CommandError {
  if (err && typeof err === 'object' && 'code' in err && 'message' in err) {
    return err as CommandError
  }
  return {
    code: 'unknown',
    message: err instanceof Error ? err.message : String(err),
  }
}

function asPickedPath(selected: unknown): string | null {
  if (typeof selected === 'string' && selected.length > 0) return selected
  if (Array.isArray(selected) && selected.length > 0) return asPickedPath(selected[0])
  if (selected && typeof selected === 'object' && 'path' in selected) {
    const path = (selected as { path: unknown }).path
    if (typeof path === 'string' && path.length > 0) return path
  }
  return null
}

export async function vaultStatus(): Promise<VaultStatus> {
  if (!isTauri()) {
    return 'uninitialized'
  }
  return invoke<VaultStatus>('vault_status')
}

export async function vaultInit(password: string): Promise<VaultStatus> {
  if (!isTauri()) {
    throw asCommandError(new Error('Vault commands require the desktop app'))
  }
  try {
    return await invoke<VaultStatus>('vault_init', { password })
  } catch (err) {
    throw asCommandError(err)
  }
}

export async function vaultUnlock(password: string): Promise<VaultStatus> {
  if (!isTauri()) {
    throw asCommandError(new Error('Vault commands require the desktop app'))
  }
  try {
    return await invoke<VaultStatus>('vault_unlock', { password })
  } catch (err) {
    throw asCommandError(err)
  }
}

export async function vaultChangePassword(
  oldPassword: string,
  newPassword: string,
): Promise<VaultStatus> {
  if (!isTauri()) {
    throw asCommandError(new Error('Vault commands require the desktop app'))
  }
  try {
    return await invoke<VaultStatus>('vault_change_password', {
      old: oldPassword,
      new: newPassword,
    })
  } catch (err) {
    throw asCommandError(err)
  }
}

export async function vaultTouch(): Promise<void> {
  if (!isTauri()) {
    return
  }
  await invoke<void>('vault_touch')
}

export async function vaultLock(): Promise<VaultStatus> {
  if (!isTauri()) {
    return 'locked'
  }
  try {
    return await invoke<VaultStatus>('vault_lock')
  } catch (err) {
    throw asCommandError(err)
  }
}

export async function appInfo(): Promise<AppInfo> {
  if (!isTauri()) {
    return { name: 'Oikonomia', version: '0.1.0-dev' }
  }
  return invoke<AppInfo>('app_info')
}

/** Native Save dialog lives in Rust. `null` means the user cancelled. */
export async function vaultBackup(): Promise<string | null> {
  if (!isTauri()) {
    throw asCommandError(new Error('Vault commands require the desktop app'))
  }
  try {
    return await invoke<string | null>('vault_backup')
  } catch (err) {
    throw asCommandError(err)
  }
}

export type BackupPick =
  | { kind: 'picked'; path: string }
  | { kind: 'cancelled' }
  | { kind: 'unavailable' }

/**
 * Native Open for restore. Prefer this before ConfirmDialog, then pass `path`
 * into {@link vaultRestore}. `cancelled` means the user dismissed the picker.
 * `unavailable` (browser / plugin failure) lets the caller omit `path` so Rust
 * can still show Open as a fallback.
 */
export async function pickVaultBackup(): Promise<BackupPick> {
  if (!isTauri()) {
    return { kind: 'unavailable' }
  }
  try {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: BACKUP_FILTER.name, extensions: [...BACKUP_FILTER.extensions] }],
    })
    const path = asPickedPath(selected)
    if (path === null) return { kind: 'cancelled' }
    return { kind: 'picked', path }
  } catch {
    return { kind: 'unavailable' }
  }
}

/**
 * Restore a backup archive. Pass `path` after a successful {@link pickVaultBackup}.
 * Omit `path` only when the web picker is unavailable. `null` means the user
 * cancelled a Rust-side Open fallback.
 */
export async function vaultRestore(opts: {
  path?: string
  replace: boolean
}): Promise<string | null> {
  if (!isTauri()) {
    throw asCommandError(new Error('Vault commands require the desktop app'))
  }
  try {
    const payload: { replace: boolean; path?: string } = { replace: opts.replace }
    if (opts.path !== undefined) {
      payload.path = opts.path
    }
    return await invoke<string | null>('vault_restore', payload)
  } catch (err) {
    throw asCommandError(err)
  }
}
