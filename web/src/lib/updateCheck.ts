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
 * Who put the dialog on Installing.
 *
 * `check` is an `update_check` Rust refused because an install is already
 * running. The dialog polls that one until the machine leaves Installing.
 * `local` is `update_install` started from this screen. That install must
 * not poll: the screen already owns the outcome.
 */
export type InstallingOrigin = 'check' | 'local'

/**
 * Full dialog state. `idle` and `checking` are local chrome.
 *
 * `installing` carries {@link InstallingOrigin} so a refused check can be
 * polled and a self-started install cannot. The origin is the variant, not
 * a flag beside it. The terminal kinds are the Rust enum. A discriminated
 * union so a leftover `version` cannot survive into Checking or Failed.
 *
 * `updateError` is the refused-check poll hitting its cap. It uses Failed's
 * title and Close, and the shared `error.update` sentence as the body.
 */
export type UpdateUiState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'installing'; origin: InstallingOrigin }
  | UpdateCheckResult
  | { kind: 'updateError' }

/**
 * How often to re-ask after a check was refused.
 *
 * The check that opened the dialog just heard Installing, so the first
 * repeat waits one interval. Two seconds is long enough not to hammer IPC
 * during a download, and short enough that a failed install becomes a
 * dismissible result without a long pause.
 */
export const REFUSED_CHECK_POLL_INTERVAL_MS = 2_000

/**
 * Stop re-asking. Past this the install has not reported a result (it can
 * sit on Installing when a restart never comes). The dialog then shows
 * {@link UpdateUiState} `updateError`: Failed's title and Close, and the
 * existing `error.update` sentence.
 */
export const REFUSED_CHECK_POLL_CAP_MS = 60_000

const DEV_PREVIEW_VERSION = '0.1.1'

export function isAvailableUpdate(value: UpdateUiState): value is AvailableUpdate {
  return value.kind === 'available' && value.version.trim() !== ''
}

/**
 * Wire payload after decode. `idle` means a non-terminal Rust kind
 * (`idle` / `checking`) — not a finished check, and not Failed.
 * `installing` is Rust's variant, with no origin: the machine is installing,
 * which is also what a refused check returns. The UI adds
 * {@link InstallingOrigin} only when it applies that result.
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

/**
 * Dialog state for a value `update_check` returned.
 *
 * Wire `installing` has no origin. It only arrives when Rust refused the
 * check, so this marks it `check` and the dialog polls it. An install this
 * screen starts is `{ origin: 'local' }` and does not pass through here.
 *
 * A `failed` status is returned as decoded, `code` included when Rust named
 * one. The dialog then words the cause the same way it does for a check
 * that failed on the first ask.
 */
export function dialogStateFromCheck(result: ParsedIpcUpdate): UpdateUiState {
  switch (result.kind) {
    case 'installing':
      return { kind: 'installing', origin: 'check' }
    case 'idle':
      return result
    case 'upToDate':
      return result
    case 'available':
      return result
    case 'availableManually':
      return result
    case 'failed':
      return result
  }
}

/**
 * True only for a refused check. A local install and every other frame
 * are not polled. Exhaustive so a new origin has to choose.
 */
export function pollsRefusedInstall(
  state: UpdateUiState,
): state is { kind: 'installing'; origin: 'check' } {
  switch (state.kind) {
    case 'installing':
      switch (state.origin) {
        case 'check':
          return true
        case 'local':
          return false
      }
    case 'idle':
      return false
    case 'checking':
      return false
    case 'upToDate':
      return false
    case 'available':
      return false
    case 'availableManually':
      return false
    case 'failed':
      return false
    case 'updateError':
      return false
  }
}

/**
 * True while Rust is still answering the check with Installing.
 * Every other decoded kind is a result the dialog can render.
 */
function installStillRefusesCheck(result: ParsedIpcUpdate): boolean {
  switch (result.kind) {
    case 'installing':
      return true
    case 'idle':
      return false
    case 'upToDate':
      return false
    case 'available':
      return false
    case 'availableManually':
      return false
    case 'failed':
      return false
  }
}

/**
 * Re-ask `ask` until it returns something other than Installing, the caller
 * cancels, or {@link REFUSED_CHECK_POLL_CAP_MS} elapses.
 *
 * The next ask is scheduled only after the previous one settles, so two
 * polls are never in flight. A result that arrives after cancel (unmount,
 * or the dialog already left this state) is dropped. At the cap `onCap`
 * runs once; a later answer does not.
 */
export function watchRefusedInstall(
  ask: () => Promise<ParsedIpcUpdate>,
  onResult: (result: ParsedIpcUpdate) => void,
  onCap: () => void,
): () => void {
  let cancelled = false
  const startedAt = Date.now()
  let pollTimer: number | undefined
  let deadlineTimer: number | undefined

  const stopTimers = () => {
    if (pollTimer !== undefined) window.clearTimeout(pollTimer)
    if (deadlineTimer !== undefined) window.clearTimeout(deadlineTimer)
    pollTimer = undefined
    deadlineTimer = undefined
  }

  const stop = () => {
    cancelled = true
    stopTimers()
  }

  const finish = (result: ParsedIpcUpdate) => {
    if (cancelled) return
    stop()
    onResult(result)
  }

  const giveUp = () => {
    if (cancelled) return
    stop()
    onCap()
  }

  const askAgain = () => {
    if (cancelled) return
    if (Date.now() - startedAt >= REFUSED_CHECK_POLL_CAP_MS) return

    void ask().then(
      (result) => {
        if (cancelled) return
        if (installStillRefusesCheck(result)) {
          if (Date.now() - startedAt >= REFUSED_CHECK_POLL_CAP_MS) return
          pollTimer = window.setTimeout(askAgain, REFUSED_CHECK_POLL_INTERVAL_MS)
          return
        }
        finish(result)
      },
      () => {
        // The check rejected. A throw carries no cause code, so this is the
        // bare failed status. It must still replace Installing.
        finish({ kind: 'failed' })
      },
    )
  }

  deadlineTimer = window.setTimeout(() => {
    giveUp()
  }, REFUSED_CHECK_POLL_CAP_MS)

  pollTimer = window.setTimeout(askAgain, REFUSED_CHECK_POLL_INTERVAL_MS)

  return () => {
    cancelled = true
    stopTimers()
  }
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
      // Paint only. `local` so the preview does not poll the stub and leave the frame.
      return { kind: 'installing', origin: 'local' }
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
