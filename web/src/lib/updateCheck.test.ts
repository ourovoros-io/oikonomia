/** @vitest-environment jsdom */

import { afterEach, describe, expect, test } from 'vitest'
import {
  dialogStateFromCheck,
  downloadPercent,
  formatDecimalMegabytes,
  isAvailableUpdate,
  parseInstallCommandResult,
  parseInstallProgress,
  parseUpdateCheckResult,
  parseUpdateNotice,
  pollsRefusedInstall,
  readDevUnlockUpdatePreview,
  readDevUpdateNotice,
  reduceUpdate,
  stubUpdateCheckResult,
  type UpdateUiState,
} from './updateCheck'
import { wholePixelShift } from './wholePixel'
import { LOCALES } from './i18n'
import { UPDATE_DIALOG_BUTTON_WIDTHS } from './updateDialogButtons'

afterEach(() => {
  window.history.replaceState({}, '', '/')
})

describe('parseUpdateCheckResult', () => {
  test('keeps only the Rust enum shapes', () => {
    expect(parseUpdateCheckResult({ kind: 'upToDate' })).toEqual({ kind: 'upToDate' })
    expect(parseUpdateCheckResult({ kind: 'failed' })).toEqual({ kind: 'failed' })
    expect(parseUpdateCheckResult({ kind: 'available', version: '0.1.1' })).toEqual({
      kind: 'available',
      version: '0.1.1',
    })
  })

  test('a failure keeps the code of its cause', () => {
    expect(parseUpdateCheckResult({ kind: 'failed', code: 'update_network' })).toEqual({
      kind: 'failed',
      code: 'update_network',
    })
    expect(
      parseUpdateCheckResult({ kind: 'failed', code: 'update_manifest_signature' }),
    ).toEqual({ kind: 'failed', code: 'update_manifest_signature' })
  })

  test('a failure without a usable code is still a failure', () => {
    const bare = parseUpdateCheckResult({ kind: 'failed' })

    expect(bare).toEqual({ kind: 'failed' })
    expect(bare).not.toHaveProperty('code')
    expect(parseUpdateCheckResult({ kind: 'failed', code: null })).toEqual({ kind: 'failed' })
    expect(parseUpdateCheckResult({ kind: 'failed', code: '' })).toEqual({ kind: 'failed' })
    expect(parseUpdateCheckResult({ kind: 'failed', code: 7 })).toEqual({ kind: 'failed' })
  })

  test('a package-managed copy gets the version but is not installable', () => {
    const parsed = parseUpdateCheckResult({
      kind: 'available_manually',
      version: '0.2.0',
      notes: 'dropped',
    })

    expect(parsed).toEqual({ kind: 'availableManually', version: '0.2.0' })
    expect(isAvailableUpdate(parsed)).toBe(false)
    expect(parseUpdateCheckResult({ kind: 'available_manually', version: ' ' })).toEqual({
      kind: 'failed',
    })
  })

  test('accepts published snake_case up_to_date', () => {
    expect(parseUpdateCheckResult({ kind: 'up_to_date' })).toEqual({ kind: 'upToDate' })
  })

  test('available+notes keeps version and discards notes', () => {
    const parsed = parseUpdateCheckResult({
      kind: 'available',
      version: '0.1.1',
      notes: 'sanitized plain text from the feed',
    })
    expect(parsed).toEqual({ kind: 'available', version: '0.1.1' })
    expect(parsed).not.toHaveProperty('notes')
  })

  test('non-terminal idle and checking do not become failed', () => {
    expect(parseUpdateCheckResult({ kind: 'idle' })).toEqual({ kind: 'idle' })
    expect(parseUpdateCheckResult({ kind: 'checking', version: '9.9.9' })).toEqual({
      kind: 'idle',
    })
  })

  test('installing stays installing and carries no version', () => {
    expect(parseUpdateCheckResult({ kind: 'installing', version: '9.9.9' })).toEqual({
      kind: 'installing',
    })
  })

  test('drops feed fields the webview must not render', () => {
    const parsed = parseUpdateCheckResult({
      kind: 'available',
      version: '0.1.1',
      notes: 'FEED NOTES',
      notesLabel: 'What’s new',
      size: '12 MB',
      url: 'https://example.invalid/app.tgz',
    })
    expect(parsed).toEqual({ kind: 'available', version: '0.1.1' })
    expect(parsed).not.toHaveProperty('url')
    expect(parsed).not.toHaveProperty('notes')
    expect(parsed).not.toHaveProperty('size')
  })

  test('unknown or empty payloads become failed', () => {
    expect(parseUpdateCheckResult(null)).toEqual({ kind: 'failed' })
    expect(parseUpdateCheckResult('available')).toEqual({ kind: 'failed' })
    expect(parseUpdateCheckResult({ kind: 'available', version: '   ' })).toEqual({
      kind: 'failed',
    })
    expect(parseUpdateCheckResult({ kind: 'downloading' })).toEqual({ kind: 'failed' })
  })
})

describe('isAvailableUpdate', () => {
  test('only the available variant with a version is installable', () => {
    expect(isAvailableUpdate({ kind: 'available', version: '1.0.0' })).toBe(true)
    expect(isAvailableUpdate({ kind: 'upToDate' })).toBe(false)
    expect(isAvailableUpdate({ kind: 'failed' })).toBe(false)
    expect(isAvailableUpdate({ kind: 'checking' })).toBe(false)
    expect(isAvailableUpdate({ kind: 'idle' })).toBe(false)
    expect(isAvailableUpdate({ kind: 'installing', origin: 'local' })).toBe(false)
    expect(isAvailableUpdate({ kind: 'installing', origin: 'check' })).toBe(false)
  })
})

describe('dialog state from a check', () => {
  test('wire installing gains a check origin and other kinds pass through', () => {
    expect(parseUpdateCheckResult({ kind: 'installing' })).toEqual({ kind: 'installing' })
    expect(dialogStateFromCheck({ kind: 'installing' })).toEqual({
      kind: 'installing',
      origin: 'check',
    })
    expect(dialogStateFromCheck({ kind: 'upToDate' })).toEqual({ kind: 'upToDate' })
    expect(dialogStateFromCheck({ kind: 'failed' })).toEqual({ kind: 'failed' })
    expect(dialogStateFromCheck({ kind: 'failed', code: 'update_network' })).toEqual({
      kind: 'failed',
      code: 'update_network',
    })
    expect(dialogStateFromCheck({ kind: 'available', version: '1.2.3' })).toEqual({
      kind: 'available',
      version: '1.2.3',
    })
    expect(dialogStateFromCheck({ kind: 'idle' })).toEqual({ kind: 'idle' })
  })

  test('only a refused check is polled', () => {
    expect(pollsRefusedInstall({ kind: 'installing', origin: 'check' })).toBe(true)
    expect(pollsRefusedInstall({ kind: 'installing', origin: 'local' })).toBe(false)
    expect(pollsRefusedInstall({ kind: 'checking' })).toBe(false)
    expect(pollsRefusedInstall({ kind: 'failed' })).toBe(false)
    expect(pollsRefusedInstall({ kind: 'failed', code: 'update_network' })).toBe(false)
    expect(pollsRefusedInstall({ kind: 'updateError' })).toBe(false)
    expect(pollsRefusedInstall({ kind: 'upToDate' })).toBe(false)
  })

  test('the installing paint hook is the local frame', () => {
    window.history.replaceState({}, '', '/?unlockUpdate=installing')
    const preview = readDevUnlockUpdatePreview()
    expect(preview).toEqual({ kind: 'installing', origin: 'local' })
    if (preview) expect(pollsRefusedInstall(preview)).toBe(false)
  })
})

describe('DEV unlockUpdate query', () => {
  test('maps each Designer frame and stays unused without the query', () => {
    expect(readDevUnlockUpdatePreview()).toBeNull()
    expect(stubUpdateCheckResult()).toEqual({ kind: 'upToDate' })

    window.history.replaceState({}, '', '/?unlockUpdate=available')
    expect(readDevUnlockUpdatePreview()).toEqual({
      kind: 'available',
      version: '0.1.4',
    })
    expect(stubUpdateCheckResult()).toEqual({ kind: 'available', version: '0.1.4' })

    window.history.replaceState({}, '', '/?unlockUpdate=idle')
    expect(readDevUnlockUpdatePreview()).toEqual({ kind: 'idle' })

    window.history.replaceState({}, '', '/?unlockUpdate=checking')
    expect(readDevUnlockUpdatePreview()).toEqual({ kind: 'checking' })
    expect(stubUpdateCheckResult()).toEqual({ kind: 'upToDate' })

    window.history.replaceState({}, '', '/?unlockUpdate=downloading')
    expect(readDevUnlockUpdatePreview()).toEqual({
      kind: 'downloading',
      version: '0.1.4',
      received: 9_600_000,
      total: 24_000_000,
    })

    window.history.replaceState({}, '', '/?unlockUpdate=downloading-unknown')
    expect(readDevUnlockUpdatePreview()).toEqual({
      kind: 'downloading',
      version: '0.1.4',
      received: 9_600_000,
      total: null,
    })

    window.history.replaceState({}, '', '/?unlockUpdate=updated')
    expect(readDevUnlockUpdatePreview()).toBeNull()
    expect(readDevUpdateNotice()).toEqual({ from: '0.1.3', to: '0.1.4' })
    expect(hasDevQueryLocked()).toBe(true)
  })
})

function hasDevQueryLocked(): boolean {
  return new URLSearchParams(window.location.search).has('unlockUpdate')
}

describe('reduceUpdate', () => {
  const idle: UpdateUiState = { kind: 'idle' }
  const checking: UpdateUiState = { kind: 'checking' }
  const downloading: UpdateUiState = {
    kind: 'downloading',
    version: '0.1.4',
    received: 10,
    total: null,
  }
  const installingLocal: UpdateUiState = { kind: 'installing', origin: 'local' }
  const installingCheck: UpdateUiState = { kind: 'installing', origin: 'check' }

  test('a check only starts from idle, and cancel or a result only lands from checking', () => {
    expect(reduceUpdate(idle, { type: 'startCheck' })).toEqual({ kind: 'checking' })
    expect(reduceUpdate(checking, { type: 'startCheck' })).toBe(checking)
    expect(reduceUpdate(downloading, { type: 'startCheck' })).toBe(downloading)
    expect(reduceUpdate(checking, { type: 'dismiss' })).toEqual({ kind: 'idle' })
    expect(reduceUpdate(checking, { type: 'checkResult', result: { kind: 'upToDate' } })).toEqual({
      kind: 'upToDate',
    })
    expect(
      reduceUpdate(checking, { type: 'checkResult', result: { kind: 'installing' } }),
    ).toEqual({ kind: 'installing', origin: 'check' })
    expect(
      reduceUpdate(checking, {
        type: 'checkResult',
        result: { kind: 'available', version: '0.1.4' },
      }),
    ).toEqual({ kind: 'available', version: '0.1.4' })
    expect(reduceUpdate(idle, { type: 'checkResult', result: { kind: 'failed' } })).toBe(idle)
  })

  test('download progress and installing are reachable only from the transfer', () => {
    const offer: UpdateUiState = { kind: 'available', version: '0.1.4' }
    const started = reduceUpdate(offer, { type: 'beginDownload' })
    expect(started).toEqual({
      kind: 'downloading',
      version: '0.1.4',
      received: 0,
      total: null,
    })
    expect(
      reduceUpdate({ kind: 'availableManually', version: '0.1.4' }, { type: 'beginDownload' }),
    ).toEqual({
      kind: 'availableManually',
      version: '0.1.4',
    })

    const unknown = reduceUpdate(started, {
      type: 'progress',
      progress: { kind: 'downloading', received: 9_600_000, total: null },
    })
    expect(unknown).toEqual({
      kind: 'downloading',
      version: '0.1.4',
      received: 9_600_000,
      total: null,
    })
    const known = reduceUpdate(unknown, {
      type: 'progress',
      progress: { kind: 'downloading', received: 9_600_000, total: 24_000_000 },
    })
    expect(known).toEqual({
      kind: 'downloading',
      version: '0.1.4',
      received: 9_600_000,
      total: 24_000_000,
    })
    const cached = reduceUpdate(started, {
      type: 'progress',
      progress: { kind: 'downloading', received: 24_000_000, total: 24_000_000 },
    })
    expect(cached).toEqual({
      kind: 'downloading',
      version: '0.1.4',
      received: 24_000_000,
      total: 24_000_000,
    })
    expect(downloadPercent(24_000_000, 24_000_000)).toBe(100)
    expect(reduceUpdate(started, {
      type: 'progress',
      progress: { kind: 'downloading', received: 0, total: 24_000_000 },
    })).toEqual({
      kind: 'downloading',
      version: '0.1.4',
      received: 0,
      total: 24_000_000,
    })

    expect(reduceUpdate(known, { type: 'progress', progress: { kind: 'installing' } })).toEqual({
      kind: 'installing',
      origin: 'local',
    })
    expect(reduceUpdate(idle, { type: 'progress', progress: { kind: 'installing' } })).toBe(idle)
    expect(
      reduceUpdate(installingCheck, { type: 'progress', progress: { kind: 'installing' } }),
    ).toBe(
      installingCheck,
    )
  })

  test('illegal transitions are rejected and cancel returns to the offer', () => {
    expect(reduceUpdate(downloading, { type: 'dismiss' })).toBe(downloading)
    expect(reduceUpdate(installingLocal, { type: 'dismiss' })).toBe(installingLocal)
    expect(reduceUpdate(installingLocal, { type: 'downloadAborted' })).toBe(installingLocal)
    expect(reduceUpdate(downloading, { type: 'downloadAborted' })).toEqual({
      kind: 'available',
      version: '0.1.4',
    })
    expect(reduceUpdate(idle, { type: 'downloadAborted' })).toBe(idle)

    expect(
      reduceUpdate(downloading, { type: 'installResult', result: { kind: 'cancelled' } }),
    ).toEqual({ kind: 'available', version: '0.1.4' })
    expect(
      reduceUpdate(installingLocal, { type: 'installResult', result: { kind: 'cancelled' } }),
    ).toBe(installingLocal)
    expect(
      reduceUpdate(downloading, {
        type: 'installResult',
        result: { kind: 'failed', code: 'update_network' },
      }),
    ).toEqual({ kind: 'failed', code: 'update_network' })
    expect(
      reduceUpdate(installingLocal, { type: 'installResult', result: { kind: 'idle' } }),
    ).toBe(installingLocal)

    expect(reduceUpdate({ kind: 'upToDate' }, { type: 'dismiss' })).toEqual({ kind: 'idle' })
    expect(reduceUpdate({ kind: 'updateError' }, { type: 'pollCap' })).toEqual({
      kind: 'updateError',
    })
    expect(reduceUpdate(installingLocal, { type: 'pollCap' })).toBe(installingLocal)
    expect(
      reduceUpdate(installingLocal, { type: 'pollResult', result: { kind: 'upToDate' } }),
    ).toBe(
      installingLocal,
    )
    expect(reduceUpdate(installingCheck, { type: 'pollCap' })).toEqual({ kind: 'updateError' })
    expect(
      reduceUpdate(installingCheck, { type: 'pollResult', result: { kind: 'upToDate' } }),
    ).toEqual({ kind: 'upToDate' })
  })
})

describe('progress numbers', () => {
  test('percent is a floored whole number and megabytes use one decimal', () => {
    expect(downloadPercent(9_600_000, 24_000_000)).toBe(40)
    expect(downloadPercent(9_600_000, null)).toBeNull()
    expect(downloadPercent(24_000_000, 24_000_000)).toBe(100)
    expect(downloadPercent(30_000_000, 24_000_000)).toBe(100)
    expect(formatDecimalMegabytes(9_600_000, 'en')).toBe('9.6')
    expect(formatDecimalMegabytes(24_000_000, 'en')).toBe('24.0')
    expect(formatDecimalMegabytes(9_600_000, 'fr')).toBe('9,6')
    expect(formatDecimalMegabytes(9_600_000, 'de')).toBe('9,6')
    expect(formatDecimalMegabytes(9_600_000, 'el')).toBe('9,6')
  })

  test('a channel sample keeps bytes and drops anything else', () => {
    expect(
      parseInstallProgress({
        kind: 'downloading',
        received: 10,
        total: 20,
        notes: 'nope',
        url: 'https://example.invalid',
      }),
    ).toEqual({ kind: 'downloading', received: 10, total: 20 })
    expect(parseInstallProgress({ kind: 'downloading', received: 0, total: null })).toEqual({
      kind: 'downloading',
      received: 0,
      total: null,
    })
    expect(
      parseInstallProgress({ kind: 'downloading', received: 24_000_000, total: 24_000_000 }),
    ).toEqual({
      kind: 'downloading',
      received: 24_000_000,
      total: 24_000_000,
    })
    expect(parseInstallProgress({ kind: 'installing', version: '9' })).toEqual({
      kind: 'installing',
    })
    expect(parseInstallProgress({ kind: 'downloading', received: -1, total: 1 })).toBeNull()
    expect(parseInstallProgress({ kind: 'downloading', received: 1 })).toBeNull()
    expect(parseInstallCommandResult({ kind: 'cancelled', version: '0.1.4' })).toEqual({
      kind: 'cancelled',
    })
    expect(parseInstallCommandResult({ kind: 'failed', code: 'update_network' })).toEqual({
      kind: 'failed',
      code: 'update_network',
    })
    expect(parseUpdateNotice({ from: '0.1.3', to: '0.1.4', notes: 'x' })).toEqual({
      from: '0.1.3',
      to: '0.1.4',
    })
    expect(parseUpdateNotice(null)).toBeNull()
    expect(parseUpdateNotice({ from: ' ', to: '0.1.4' })).toBeNull()
  })
})

describe('updater button widths', () => {
  test('every locale has a width and every value is a multiple of 8', () => {
    expect(Object.keys(UPDATE_DIALOG_BUTTON_WIDTHS).sort()).toEqual([...LOCALES].sort())
    for (const locale of LOCALES) {
      const widths = UPDATE_DIALOG_BUTTON_WIDTHS[locale]
      expect(widths.secondary % 8).toBe(0)
      expect(widths.primary % 8).toBe(0)
      expect(widths.secondary).toBeGreaterThan(0)
      expect(widths.primary).toBeGreaterThan(0)
    }
  })
})

describe('whole pixel shift', () => {
  test('the measured unlock-card fraction lands on 290', () => {
    expect(wholePixelShift(290.296875)).toBeCloseTo(-0.296875)
    expect(290.296875 + wholePixelShift(290.296875)).toBe(290)
    expect(wholePixelShift(278)).toBe(0)
    expect(wholePixelShift(Number.NaN)).toBe(0)
  })
})
