/**
 * Update dialog state the webview is allowed to render.
 *
 * The webview never talks to the internet. Rust owns the updater; this file
 * only decodes what `update_check`, `update_install` and the progress channel
 * return. Extra feed fields (notes, size, URL) are dropped so they cannot
 * leak into the UI. A download total is shown only when the progress channel
 * itself carries one.
 */

import type { Locale } from './i18n'

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
 * One progress sample from the install channel.
 *
 * `total` is always present: `null` when the size is still unknown, never
 * omitted. The first sample is `received: 0`. A copy already verified in
 * the cache sends one sample with `received === total`, and the bar paints
 * full from that sample. `kind` is the snake_case tag Rust sends.
 */
export type InstallProgress =
  | { kind: 'downloading'; received: number; total: number | null }
  | { kind: 'installing' }

/**
 * What `update_install` resolves to.
 *
 * The existing check/install statuses, plus `cancelled` when the transfer
 * was aborted. Rust is `available` again after that, so the dialog returns
 * to the offer and Install can be pressed without another check. A restart
 * that never returns a value stays `undefined` at the wrapper, not here.
 */
export type InstallCommandResult = ParsedIpcUpdate | { kind: 'cancelled' }

/**
 * Full dialog state. One variant per frame, so a leftover version or a
 * byte count cannot survive into a state that does not show it.
 *
 * `installing` carries {@link InstallingOrigin} so a refused check can be
 * polled and a self-started install cannot. The origin is the variant, not
 * a flag beside it. `updateError` is the refused-check poll hitting its cap.
 */
export type UpdateUiState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'upToDate' }
  | { kind: 'available'; version: string }
  | { kind: 'availableManually'; version: string }
  | { kind: 'downloading'; version: string; received: number; total: number | null }
  | { kind: 'installing'; origin: InstallingOrigin }
  | FailedUpdate
  | { kind: 'updateError' }

/**
 * Events the dialog may apply. Anything not listed for the current variant
 * is ignored, so a late progress sample cannot reopen a closed dialog and
 * a cancel cannot leave Installing.
 */
export type UpdateEvent =
  | { type: 'startCheck' }
  | { type: 'checkResult'; result: ParsedIpcUpdate }
  | { type: 'dismiss' }
  | { type: 'beginDownload' }
  | { type: 'progress'; progress: InstallProgress }
  | { type: 'installResult'; result: InstallCommandResult }
  | { type: 'downloadAborted' }
  | { type: 'pollResult'; result: ParsedIpcUpdate }
  | { type: 'pollCap' }

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

/**
 * A check result stays on Checking at least this long, so a fast answer
 * never flashes the dialog.
 */
export const CHECKING_MIN_MS = 600

/** Example versions for the DEV paint hook. 0.1.4 is not a release. */
const DEV_PREVIEW_VERSION = '0.1.4'
const DEV_CURRENT_VERSION = '0.1.3'

/** 9.6 MB of the 24.0 MB example artifact, so the preview paints 40%. */
const DEV_DOWNLOAD_RECEIVED = 9_600_000
const DEV_DOWNLOAD_TOTAL = 24_000_000

/** Decimal megabytes: 1 MB = 1,000,000 bytes, one fractional digit. */
const BYTES_PER_MEGABYTE = 1_000_000

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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isByteCount(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0
}

function nonemptyVersion(value: Record<string, unknown>): string | null {
  if (typeof value.version !== 'string') return null
  const version = value.version.trim()
  return version === '' ? null : version
}

function parseFailedStatus(value: Record<string, unknown>): FailedUpdate {
  if (typeof value.code !== 'string' || value.code === '') return { kind: 'failed' }
  return { kind: 'failed', code: value.code }
}

function parseVersioned(
  kind: 'available' | 'availableManually',
  value: Record<string, unknown>,
): ParsedIpcUpdate | null {
  const version = nonemptyVersion(value)
  if (version === null) return null
  return { kind, version }
}

/**
 * Wire kinds. `up_to_date` is the Rust name; `upToDate` is accepted so a
 * camelCase stub still decodes. Unknown kinds are not in the map.
 */
const CHECK_PARSERS: Record<string, (value: Record<string, unknown>) => ParsedIpcUpdate | null> = {
  idle: () => ({ kind: 'idle' }),
  checking: () => ({ kind: 'idle' }),
  installing: () => ({ kind: 'installing' }),
  up_to_date: () => ({ kind: 'upToDate' }),
  upToDate: () => ({ kind: 'upToDate' }),
  failed: parseFailedStatus,
  available: (value) => parseVersioned('available', value),
  available_manually: (value) => parseVersioned('availableManually', value),
  availableManually: (value) => parseVersioned('availableManually', value),
}

/**
 * Decode the published Rust enum (`tag = kind`, `rename_all = snake_case`).
 *
 * `notes`, `url`, `size`, and pubkey never enter the UI union. Non-terminal
 * `idle` / `checking` become `idle` so they cannot leak a leftover version.
 * `failed` keeps its `code` when that code is a non-empty string.
 */
export function parseUpdateCheckResult(value: unknown): ParsedIpcUpdate {
  if (!isRecord(value) || typeof value.kind !== 'string') return { kind: 'failed' }

  const parse = CHECK_PARSERS[value.kind]
  if (!parse) return { kind: 'failed' }
  return parse(value) ?? { kind: 'failed' }
}

/**
 * Decode one channel sample. A malformed sample is dropped, not painted,
 * and notes or a URL on the sample never become part of the progress value.
 *
 * `total` is required. `null` means the size is unknown; a missing field is
 * not that, so the sample is dropped rather than guessed.
 */
export function parseInstallProgress(value: unknown): InstallProgress | null {
  if (!isRecord(value)) return null

  if (value.kind === 'installing') return { kind: 'installing' }

  if (value.kind !== 'downloading' || !isByteCount(value.received)) return null
  if (!Object.hasOwn(value, 'total')) return null
  if (value.total === null) {
    return { kind: 'downloading', received: value.received, total: null }
  }
  if (!isByteCount(value.total)) return null

  return { kind: 'downloading', received: value.received, total: value.total }
}

/** Decode `update_install`'s return. `cancelled` is not a check status. */
export function parseInstallCommandResult(value: unknown): InstallCommandResult {
  if (isRecord(value) && value.kind === 'cancelled') return { kind: 'cancelled' }
  return parseUpdateCheckResult(value)
}

/** `{ from, to }` from `update_take_notice`, or null when the payload is not that pair. */
export function parseUpdateNotice(value: unknown): { from: string; to: string } | null {
  if (!isRecord(value)) return null
  if (typeof value.from !== 'string' || typeof value.to !== 'string') return null

  const from = value.from.trim()
  const to = value.to.trim()
  if (!from || !to) return null

  return { from, to }
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

type TransferEvent = Extract<
  UpdateEvent,
  { type: 'beginDownload' | 'progress' | 'installResult' | 'downloadAborted' }
>

type PollEvent = Extract<UpdateEvent, { type: 'pollResult' | 'pollCap' }>

type CheckEvent = Exclude<UpdateEvent, TransferEvent | PollEvent>

function isTransferEvent(event: UpdateEvent): event is TransferEvent {
  return (
    event.type === 'beginDownload' ||
    event.type === 'progress' ||
    event.type === 'installResult' ||
    event.type === 'downloadAborted'
  )
}

function isPollEvent(event: UpdateEvent): event is PollEvent {
  return event.type === 'pollResult' || event.type === 'pollCap'
}

function reduceTransfer(state: UpdateUiState, event: TransferEvent): UpdateUiState {
  switch (event.type) {
    case 'beginDownload':
      if (state.kind !== 'available') return state
      return { kind: 'downloading', version: state.version, received: 0, total: null }
    case 'progress':
      return applyProgress(state, event.progress)
    case 'installResult':
      return applyInstallResult(state, event.result)
    case 'downloadAborted':
      return offerAgain(state)
  }
}

function reducePoll(state: UpdateUiState, event: PollEvent): UpdateUiState {
  if (!pollsRefusedInstall(state)) return state
  if (event.type === 'pollCap') return { kind: 'updateError' }
  return dialogStateFromCheck(event.result)
}

function reduceCheck(state: UpdateUiState, event: CheckEvent): UpdateUiState {
  switch (event.type) {
    case 'startCheck':
      return state.kind === 'idle' ? { kind: 'checking' } : state
    case 'checkResult':
      return state.kind === 'checking' ? dialogStateFromCheck(event.result) : state
    case 'dismiss':
      return canDismiss(state) ? { kind: 'idle' } : state
  }
}

/**
 * Apply one event. A transition the current variant does not allow returns
 * the same state, so a late event cannot paint a frame the machine has left.
 */
export function reduceUpdate(state: UpdateUiState, event: UpdateEvent): UpdateUiState {
  if (isTransferEvent(event)) return reduceTransfer(state, event)
  if (isPollEvent(event)) return reducePoll(state, event)
  return reduceCheck(state, event)
}

/**
 * Close, Later, Cancel and Escape leave these frames.
 * Downloading aborts through its own event. Installing does not dismiss.
 */
const DISMISSABLE: Record<UpdateUiState['kind'], boolean> = {
  idle: false,
  checking: true,
  upToDate: true,
  available: true,
  availableManually: true,
  downloading: false,
  installing: false,
  failed: true,
  updateError: true,
}

function canDismiss(state: UpdateUiState): boolean {
  return DISMISSABLE[state.kind]
}

function applyProgress(state: UpdateUiState, progress: InstallProgress): UpdateUiState {
  if (state.kind === 'downloading' && progress.kind === 'downloading') {
    return {
      kind: 'downloading',
      version: state.version,
      received: progress.received,
      total: progress.total,
    }
  }
  if (state.kind === 'downloading' && progress.kind === 'installing') {
    return { kind: 'installing', origin: 'local' }
  }
  if (state.kind === 'installing' && state.origin === 'local' && progress.kind === 'installing') {
    return state
  }
  return state
}

/**
 * The offer the download started from. Cancel puts Rust back on `available`,
 * so the dialog returns there with the same version and Install works again.
 * Once Installing has started, cancel is ignored.
 */
function offerAgain(state: UpdateUiState): UpdateUiState {
  if (state.kind !== 'downloading') return state
  return { kind: 'available', version: state.version }
}

/**
 * A failure replaces the transfer and keeps the code Rust named. `cancelled`
 * returns to the offer while bytes are still moving. Any other status (the
 * app is about to restart) leaves the frame where it is. Installing is not
 * delayed here: the frame changes on the `installing` sample, and Rust holds
 * that state before it restarts.
 */
function isTransferFrame(
  state: UpdateUiState,
): state is Extract<UpdateUiState, { kind: 'downloading' | 'installing' }> {
  return state.kind === 'downloading' || state.kind === 'installing'
}

function applyInstallResult(state: UpdateUiState, result: InstallCommandResult): UpdateUiState {
  if (!isTransferFrame(state)) return state
  return finishInstall(state, result)
}

/** `failed` replaces the frame. Anything else, except cancel, leaves it. */
function finishInstall(state: UpdateUiState, result: InstallCommandResult): UpdateUiState {
  switch (result.kind) {
    case 'cancelled':
      return offerAgain(state)
    case 'failed':
      return result
    case 'idle':
    case 'installing':
    case 'upToDate':
    case 'available':
    case 'availableManually':
      return state
  }
}

/**
 * True only for a refused check. A local install is not polled.
 * The origin map is exhaustive, so a new origin has to choose.
 */
const POLL_ORIGIN: Record<InstallingOrigin, boolean> = {
  check: true,
  local: false,
}

export function pollsRefusedInstall(
  state: UpdateUiState,
): state is { kind: 'installing'; origin: 'check' } {
  if (state.kind !== 'installing') return false
  return POLL_ORIGIN[state.origin]
}

/** True while Rust is still answering the check with Installing. */
function installStillRefusesCheck(result: ParsedIpcUpdate): boolean {
  return result.kind === 'installing'
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

/**
 * Whole percent for a determinate bar, or null when the size is unknown.
 * The width is this integer, so a fractional byte ratio cannot paint a
 * sliver past the percent the label shows. Values above 100 clamp.
 */
export function downloadPercent(received: number, total: number | null): number | null {
  if (total === null || total <= 0) return null
  return Math.min(100, Math.floor((received / total) * 100))
}

/**
 * Megabytes with one decimal and the locale's decimal separator.
 * Decimal MB, matching the sizes GitHub publishes (1 MB = 1,000,000 bytes).
 */
export function formatDecimalMegabytes(bytes: number, locale: Locale): string {
  const megabytes = Math.round(bytes / (BYTES_PER_MEGABYTE / 10)) / 10
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  }).format(megabytes)
}

/** Milliseconds still to wait so Checking has been up for {@link CHECKING_MIN_MS}. */
export function checkingHoldMs(startedAt: number, now: number): number {
  return Math.max(0, CHECKING_MIN_MS - (now - startedAt))
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

/** True when a DEV `?unlockUpdate=` query is present, including the notice frame. */
export function hasDevUnlockUpdateQuery(): boolean {
  return readUnlockUpdateQuery() !== null
}

/**
 * DEV-only `?unlockUpdate=` paint hook. Returns null in production builds
 * so the query cannot open a dialog after `vite build`.
 *
 * `updated` is the relaunch notice, not a dialog, so it returns null here.
 */
/**
 * Paint frames for `?unlockUpdate=`. `updated` is the notice, not a dialog,
 * so it is absent here. `installing` is `local` so the preview does not poll.
 */
const DEV_PREVIEWS: Record<string, UpdateUiState> = {
  idle: { kind: 'idle' },
  checking: { kind: 'checking' },
  uptodate: { kind: 'upToDate' },
  available: { kind: 'available', version: DEV_PREVIEW_VERSION },
  manual: { kind: 'availableManually', version: DEV_PREVIEW_VERSION },
  downloading: {
    kind: 'downloading',
    version: DEV_PREVIEW_VERSION,
    received: DEV_DOWNLOAD_RECEIVED,
    total: DEV_DOWNLOAD_TOTAL,
  },
  'downloading-unknown': {
    kind: 'downloading',
    version: DEV_PREVIEW_VERSION,
    received: DEV_DOWNLOAD_RECEIVED,
    total: null,
  },
  failed: { kind: 'failed' },
  installing: { kind: 'installing', origin: 'local' },
}

export function readDevUnlockUpdatePreview(): UpdateUiState | null {
  const raw = readUnlockUpdateQuery()
  if (raw === null) return null
  return DEV_PREVIEWS[raw] ?? null
}

/**
 * DEV-only relaunch notice (`?unlockUpdate=updated`). Null in production
 * and for every dialog frame.
 */
export function readDevUpdateNotice(): { from: string; to: string } | null {
  if (readUnlockUpdateQuery() !== 'updated') return null
  return { from: DEV_CURRENT_VERSION, to: DEV_PREVIEW_VERSION }
}

/**
 * Version line on the up-to-date frame. The DEV preview paints 0.1.3,
 * the version the frame was drawn against. Otherwise the running app version.
 */
export function upToDateVersion(appVersion: string | null | undefined): string | null {
  if (readUnlockUpdateQuery() === 'uptodate') return DEV_CURRENT_VERSION
  const version = appVersion?.trim()
  return version ? version : null
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

let startupNotice: Promise<{ from: string; to: string } | null> | undefined

/**
 * Ask once per page load. Strict mode mounts twice; a second call would
 * read the marker after Rust had already cleared it and the notice would vanish.
 */
export function takeStartupUpdateNotice(
  ask: () => Promise<{ from: string; to: string } | null>,
): Promise<{ from: string; to: string } | null> {
  startupNotice ??= ask().catch(() => null)
  return startupNotice
}

/** Test-only. The startup notice is once per load, so tests reset the guard. */
export function resetStartupUpdateNoticeForTests(): void {
  startupNotice = undefined
}
