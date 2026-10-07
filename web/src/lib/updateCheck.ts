/**
 * Update dialog state the webview is allowed to render.
 *
 * The webview never talks to the internet. Rust owns the updater; this file
 * only decodes the enum `update_check` will return. Extra feed fields (notes,
 * size, URL) are dropped so they cannot leak into the UI.
 */

export type UpdateCheckKind = 'upToDate' | 'available' | 'availableManually' | 'failed'

/**
 * Result of `update_check`. `available` carries only the version string.
 * `availableManually` is a newer version this copy must not install itself:
 * the system package manager owns its files (a `.deb` install).
 * `failed` carries the stable code of what stopped the check or the install
 * (`update_network`, `update_manifest_signature`, ...) when Rust names one.
 * It is a code from errorCodes.json, never text, and the dialog words it.
 */
export type UpdateCheckResult =
  | { kind: 'upToDate' }
  | { kind: 'available'; version: string }
  | { kind: 'availableManually'; version: string }
  | FailedUpdate

/** A check or an install that failed, with the code of its cause when known. */
export type FailedUpdate = { kind: 'failed'; code?: string }

/** The only state from which `update_install` may be invoked. */
export type AvailableUpdate = Extract<UpdateCheckResult, { kind: 'available' }>

/**
 * Full dialog state. `idle` and `checking` are local chrome; `installing` is
 * set locally when an install starts and also arrives from Rust when a check
 * is refused because an install is in flight. The terminal kinds are the Rust
 * enum. A discriminated union so a leftover `version` cannot survive into
 * Checking or Failed.
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
 * Wire payload after decode. `idle` means a non-terminal Rust kind
 * (`idle` / `checking`) — not a finished check, and not Failed.
 * `installing` means Rust refused the check because an install is running.
 */
export type ParsedIpcUpdate = UpdateCheckResult | { kind: 'idle' } | { kind: 'installing' }

/**
 * Decode the published Rust enum (`tag = kind`, `rename_all = snake_case`).
 *
 * `up_to_date` must map to the internal `upToDate` variant. Treating the
 * snake_case kind as unknown would paint a successful check as Failed.
 * `notes`, `url`, `size`, and pubkey never enter the UI union — available
 * keeps `version` only. Non-terminal `idle` / `checking` become `idle`
 * so they cannot leak a leftover version into the form. `installing` stays
 * `installing`, so the dialog keeps showing the install in flight. `failed`
 * keeps its `code` when it is a non-empty string; a status without one, as
 * older builds send and as a check whose task died sends, is still `failed`.
 */
export function parseUpdateCheckResult(value: unknown): ParsedIpcUpdate {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return { kind: 'failed' }
  }
  const record = value as Record<string, unknown>
  const kind = record.kind
  if (kind === 'idle' || kind === 'checking') {
    return { kind: 'idle' }
  }
  if (kind === 'installing') {
    return { kind: 'installing' }
  }
  if (kind === 'up_to_date' || kind === 'upToDate') {
    return { kind: 'upToDate' }
  }
  if (kind === 'failed') {
    return typeof record.code === 'string' && record.code !== ''
      ? { kind: 'failed', code: record.code }
      : { kind: 'failed' }
  }
  if (kind === 'available' && typeof record.version === 'string') {
    const version = record.version.trim()
    if (version) return { kind: 'available', version }
  }
  if (
    (kind === 'available_manually' || kind === 'availableManually') &&
    typeof record.version === 'string'
  ) {
    const version = record.version.trim()
    if (version) return { kind: 'availableManually', version }
  }
  return { kind: 'failed' }
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
    case 'idle':
      return { kind: 'idle' }
    case 'checking':
      return { kind: 'checking' }
    case 'uptodate':
      return { kind: 'upToDate' }
    case 'available':
      return { kind: 'available', version: DEV_PREVIEW_VERSION }
    case 'manual':
      return { kind: 'availableManually', version: DEV_PREVIEW_VERSION }
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
  if (preview?.kind === 'available' || preview?.kind === 'availableManually') {
    return preview
  }
  return { kind: 'upToDate' }
}
