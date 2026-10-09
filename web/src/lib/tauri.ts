import { Channel, invoke } from '@tauri-apps/api/core'
import { asCommandError, isMissingIpcCommand } from './commandError'
import {
  hasDevUnlockUpdateQuery,
  isAvailableUpdate,
  parseInstallCommandResult,
  parseInstallProgress,
  parseUpdateCheckResult,
  parseUpdateNotice,
  readDevUnlockUpdatePreview,
  stubUpdateCheckResult,
  type AvailableUpdate,
  type InstallCommandResult,
  type InstallProgress,
  type ParsedIpcUpdate,
} from './updateCheck'

export type {
  AvailableUpdate,
  InstallCommandResult,
  InstallProgress,
  ParsedIpcUpdate,
  UpdateCheckResult,
  UpdateUiState,
} from './updateCheck'
export {
  isAvailableUpdate,
  parseInstallCommandResult,
  parseInstallProgress,
  parseUpdateCheckResult,
  parseUpdateNotice,
  readDevUnlockUpdatePreview,
}

/** The relaunch marker `update_take_notice` returns once, then clears. */
export type UpdateNotice = { from: string; to: string }

export type VaultStatus = 'uninitialized' | 'locked' | 'unlocked'

export type AppInfo = {
  version: string
  name: string
  /** Support mailbox. Carried by the native payload so the address lives in Rust. */
  support_email: string
}

export type CommandError = {
  code: string
  /** English; for logs only. The UI never shows it. */
  message: string
  /** Named values for the localized text of `code`. */
  params?: Record<string, string>
}

/** True when running inside the Tauri webview (not a plain browser tab). */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export async function vaultStatus(): Promise<VaultStatus> {
  if (!isTauri()) {
    // Designer QA of the unlock-update frames needs Welcome chrome.
    // Dead in production: the query reader is DEV-gated.
    return hasDevUnlockUpdateQuery() ? 'locked' : 'uninitialized'
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
    // Browser preview only; the shipped values come from Rust `app_info`.
    return {
      name: 'Oikonomia',
      version: '0.1.0-dev',
      support_email: 'info@ourovoros.io',
    }
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

/**
 * Native Open dialog lives in Rust. `null` means the user cancelled.
 * Confirm in the UI after a path is returned, then pass it to {@link vaultRestore}.
 */
export async function vaultPickBackup(): Promise<string | null> {
  if (!isTauri()) {
    throw asCommandError(new Error('Vault commands require the desktop app'))
  }
  try {
    return await invoke<string | null>('vault_pick_backup')
  } catch (err) {
    throw asCommandError(err)
  }
}

/**
 * Restore a backup archive. Pass `path` after {@link vaultPickBackup}.
 * Omit `path` only as a fallback so Rust can still show Open. `null` means
 * the user cancelled a Rust-side Open fallback.
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

/**
 * Ask Rust whether a new application is available. The webview only renders
 * the returned enum — it does not fetch, and it does not see a download URL.
 *
 * Until `update_check` exists on the backend, a local stub answers so paint
 * and tests stay reviewable.
 */
export async function updateCheck(): Promise<ParsedIpcUpdate> {
  if (!isTauri()) {
    return stubUpdateCheckResult()
  }
  try {
    const raw = await invoke<unknown>('update_check')
    return parseUpdateCheckResult(raw)
  } catch (err) {
    const cmd = asCommandError(err)
    if (isMissingIpcCommand(cmd, 'update_check')) {
      return stubUpdateCheckResult()
    }
    return { kind: 'failed' }
  }
}

/**
 * Install the already-checked update. Accepts only {@link AvailableUpdate}
 * so Checking / Failed / Up-to-date cannot request an install.
 *
 * The webview passes no URL, endpoint, or pubkey. Progress arrives on a
 * Tauri channel (`onProgress` → Rust `on_progress`). The channel is
 * required, so every invoke passes one. Samples are `downloading` with
 * `received` and `total` (`null` when the size is unknown, never omitted),
 * then `installing`. The command resolves to the existing update status,
 * or `{ kind: "cancelled" }` when the transfer was aborted. Rust is
 * `available` again after a cancel. A missing return (the process is
 * restarting) is `undefined`, not a failure.
 */
export async function updateInstall(
  available: AvailableUpdate,
  onProgress: (progress: InstallProgress) => void,
): Promise<InstallCommandResult | undefined> {
  if (!isAvailableUpdate(available)) {
    return undefined
  }
  if (!isTauri()) {
    return undefined
  }
  const onProgressChannel = new Channel<unknown>()
  onProgressChannel.onmessage = (raw) => {
    const progress = parseInstallProgress(raw)
    if (progress) onProgress(progress)
  }
  try {
    const raw = await invoke<unknown>('update_install', { onProgress: onProgressChannel })
    if (raw === undefined || raw === null) return undefined
    return parseInstallCommandResult(raw)
  } catch (err) {
    const cmd = asCommandError(err)
    if (isMissingIpcCommand(cmd, 'update_install')) {
      return undefined
    }
    throw cmd
  }
}

/**
 * Abort the download in progress.
 *
 * `true` means Rust aborted the download. `update_install` then resolves
 * `{ kind: "cancelled" }` and the dialog returns to the offer. `false`
 * means Installing has already started, or nothing was downloading: the
 * dialog stays where it is. Only meaningful while the UI is on Downloading.
 */
export async function updateCancel(): Promise<boolean> {
  if (!isTauri()) return false
  try {
    const raw = await invoke<unknown>('update_cancel')
    return raw === true
  } catch (err) {
    const cmd = asCommandError(err)
    if (isMissingIpcCommand(cmd, 'update_cancel')) return false
    return false
  }
}

/**
 * Read the relaunch marker once. Rust compares it with the running version
 * and clears it. Null when this launch is not the one right after an update.
 */
export async function updateTakeNotice(): Promise<UpdateNotice | null> {
  if (!isTauri()) return null
  try {
    const raw = await invoke<unknown>('update_take_notice')
    return parseUpdateNotice(raw)
  } catch (err) {
    const cmd = asCommandError(err)
    if (isMissingIpcCommand(cmd, 'update_take_notice')) return null
    return null
  }
}
