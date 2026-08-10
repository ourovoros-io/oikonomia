import { invoke } from '@tauri-apps/api/core'

export type VaultStatus = 'uninitialized' | 'locked' | 'unlocked'

export type AppInfo = {
  version: string
  name: string
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
