/**
 * Update dialog state the webview is allowed to render.
 *
 * The webview never talks to the internet. Rust owns the updater; this file
 * only decodes the enum `update_check` will return. Extra feed fields (notes,
 * size, URL) are dropped so they cannot leak into the UI.
 */

export type UpdateCheckKind = 'upToDate' | 'available' | 'failed'

/** Result of `update_check`. `available` carries only the version string. */
export type UpdateCheckResult =
  | { kind: 'upToDate' }
  | { kind: 'available'; version: string }
  | { kind: 'failed' }

/** The only state from which `update_install` may be invoked. */
export type AvailableUpdate = Extract<UpdateCheckResult, { kind: 'available' }>

/**
 * Full dialog state. `idle` / `checking` / `installing` are local chrome;
 * the terminal kinds are the Rust enum. A discriminated union so a leftover
 * `version` cannot survive into Checking or Failed.
 */
export type UpdateUiState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'installing' }
  | UpdateCheckResult

const DEV_PREVIEW_VERSION = '0.1.1'

export function isAvailableUpdate(value: UpdateUiState): value is AvailableUpdate {
  return value.kind === 'available' && value.version.trim() !== ''
}

/**
 * Decode an IPC payload into the update enum. Unknown shapes and extra
 * fields become `failed` or a version-only `available` — never a URL.
 */
export function parseUpdateCheckResult(value: unknown): UpdateCheckResult {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return { kind: 'failed' }
  }
  const record = value as Record<string, unknown>
  if (record.kind === 'upToDate') {
    return { kind: 'upToDate' }
  }
  if (record.kind === 'failed') {
    return { kind: 'failed' }
  }
  if (record.kind === 'available' && typeof record.version === 'string') {
    const version = record.version.trim()
    if (version) return { kind: 'available', version }
  }
  return { kind: 'failed' }
}

/** Tauri reports an unregistered command; the FE stub may answer instead. */
export function isMissingIpcCommand(err: { code: string; message: string }, command: string): boolean {
  const haystack = `${err.code} ${err.message}`.toLowerCase()
  const name = command.toLowerCase()
  return haystack.includes(name) && (haystack.includes('not found') || haystack.includes('unknown'))
}

function readUnlockUpdateQuery(): string | null {
  if (!import.meta.env.DEV) return null
  if (typeof window === 'undefined') return null
  try {
    return new URLSearchParams(window.location.search).get('unlockUpdate')
  } catch {
    return null
  }
}

/**
 * DEV-only `?unlockUpdate=` paint hook. Returns null in production builds
 * so the query cannot open a dialog after `vite build`.
 */
export function readDevUnlockUpdatePreview(): UpdateUiState | null {
  const raw = readUnlockUpdateQuery()
  switch (raw) {
    case 'checking':
      return { kind: 'checking' }
    case 'uptodate':
      return { kind: 'upToDate' }
    case 'available':
      return { kind: 'available', version: DEV_PREVIEW_VERSION }
    case 'failed':
      return { kind: 'failed' }
    case 'installing':
      return { kind: 'installing' }
    default:
      return null
  }
}

/**
 * Local answer when Rust has not registered `update_check` yet.
 * Designer can pick a terminal result via `?unlockUpdate=`.
 */
export function stubUpdateCheckResult(): UpdateCheckResult {
  const preview = readDevUnlockUpdatePreview()
  if (preview?.kind === 'upToDate' || preview?.kind === 'failed') {
    return preview
  }
  if (preview?.kind === 'available') {
    return preview
  }
  return { kind: 'upToDate' }
}
