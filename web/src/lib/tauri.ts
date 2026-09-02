import { invoke } from '@tauri-apps/api/core'
import {
  isAvailableUpdate,
  isMissingIpcCommand,
  parseUpdateCheckResult,
  readDevUnlockUpdatePreview,
  stubUpdateCheckResult,
  type AvailableUpdate,
  type ParsedIpcUpdate,
} from './updateCheck'

export type { AvailableUpdate, ParsedIpcUpdate, UpdateCheckResult, UpdateUiState } from './updateCheck'
export { isAvailableUpdate, parseUpdateCheckResult, readDevUnlockUpdatePreview }

export type VaultStatus = 'uninitialized' | 'locked' | 'unlocked'

export type AppInfo = {
  version: string
  name: string
  /** Support mailbox. Carried by the native payload so the address lives in Rust. */
  support_email: string
  /** `mailto:` link for the support mailbox; the app version is already in the subject. */
  support_mailto: string
}

export type CommandError = {
  code: string
  message: string
}

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

export async function vaultStatus(): Promise<VaultStatus> {
  if (!isTauri()) {
    // Designer QA of the six unlock-update frames needs Welcome chrome.
    // Dead in production: `readDevUnlockUpdatePreview` is DEV-gated.
    return readDevUnlockUpdatePreview() ? 'locked' : 'uninitialized'
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
      support_mailto: 'mailto:info@ourovoros.io?subject=Oikonomia%20v0.1.0-dev%20support',
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
 * The webview passes no URL, endpoint, or pubkey. On success Rust restarts
 * the app. On `{ kind: "failed" }` the UI shows Failed and unlock stays usable.
 */
export async function updateInstall(
  available: AvailableUpdate,
): Promise<{ kind: 'failed' } | undefined> {
  if (!isAvailableUpdate(available)) {
    return undefined
  }
  if (!isTauri()) {
    return undefined
  }
  try {
    const raw = await invoke<unknown>('update_install')
    const parsed = parseUpdateCheckResult(raw)
    if (parsed.kind === 'failed') return { kind: 'failed' }
    return undefined
  } catch (err) {
    const cmd = asCommandError(err)
    if (isMissingIpcCommand(cmd, 'update_install')) {
      return undefined
    }
    throw cmd
  }
}
