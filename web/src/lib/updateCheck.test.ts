/** @vitest-environment jsdom */

import { afterEach, describe, expect, test } from 'vitest'
import {
  isAvailableUpdate,
  parseUpdateCheckResult,
  readDevUnlockUpdatePreview,
  stubUpdateCheckResult,
} from './updateCheck'

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
    expect(isAvailableUpdate({ kind: 'installing' })).toBe(false)
  })
})

describe('DEV unlockUpdate query', () => {
  test('maps each Designer frame and stays unused without the query', () => {
    expect(readDevUnlockUpdatePreview()).toBeNull()
    expect(stubUpdateCheckResult()).toEqual({ kind: 'upToDate' })

    window.history.replaceState({}, '', '/?unlockUpdate=available')
    expect(readDevUnlockUpdatePreview()).toEqual({
      kind: 'available',
      version: '0.1.1',
    })
    expect(stubUpdateCheckResult()).toEqual({ kind: 'available', version: '0.1.1' })

    window.history.replaceState({}, '', '/?unlockUpdate=idle')
    expect(readDevUnlockUpdatePreview()).toEqual({ kind: 'idle' })

    window.history.replaceState({}, '', '/?unlockUpdate=checking')
    expect(readDevUnlockUpdatePreview()).toEqual({ kind: 'checking' })
    expect(stubUpdateCheckResult()).toEqual({ kind: 'upToDate' })
  })
})
