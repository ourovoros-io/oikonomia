/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

const invoke = vi.fn()
const channels: Array<{ onmessage: (value: unknown) => void }> = []

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  Channel: class Channel {
    onmessage: (value: unknown) => void = () => {}

    constructor() {
      channels.push(this)
    }
  },
}))

import { updateCancel, updateCheck, updateInstall, updateTakeNotice } from './tauri'

afterEach(() => {
  invoke.mockReset()
  channels.length = 0
  window.history.replaceState({}, '', '/')
  Reflect.deleteProperty(window, '__TAURI_INTERNALS__')
})

beforeEach(() => {
  invoke.mockReset()
})

describe('updateCheck / updateInstall wrappers', () => {
  test('browser stub does not invoke and never returns a URL', async () => {
    const result = await updateCheck()
    expect(invoke).not.toHaveBeenCalled()
    expect(result).toEqual({ kind: 'upToDate' })
    expect(result).not.toHaveProperty('url')
  })

  test('missing update_check command uses the local stub', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
    invoke.mockRejectedValue({ code: 'unknown', message: 'command update_check not found' })
    await expect(updateCheck()).resolves.toEqual({ kind: 'upToDate' })
  })

  test('registered update_check accepts snake_case up_to_date', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
    invoke.mockResolvedValue({ kind: 'up_to_date' })
    await expect(updateCheck()).resolves.toEqual({ kind: 'upToDate' })
  })

  test('registered update_check is parsed and extra feed fields are dropped', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
    invoke.mockResolvedValue({
      kind: 'available',
      version: '0.2.0',
      url: 'https://example.invalid/app.tgz',
      notes: 'nope',
    })
    await expect(updateCheck()).resolves.toEqual({ kind: 'available', version: '0.2.0' })
    expect(invoke).toHaveBeenCalledWith('update_check')
  })

  test('update_install invokes only when given Available', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
    invoke.mockResolvedValue(undefined)
    await updateInstall({ kind: 'available', version: '0.1.1' }, () => undefined)
    expect(invoke).toHaveBeenCalledWith('update_install', { onProgress: channels[0] })
  })

  test('a channel streams downloading then installing, and extra fields are dropped', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
    invoke.mockReturnValue(new Promise(() => undefined))
    const seen: unknown[] = []

    void updateInstall({ kind: 'available', version: '0.1.4' }, (progress) => {
      seen.push(progress)
    })

    expect(channels).toHaveLength(1)
    channels[0]?.onmessage({
      kind: 'downloading',
      received: 9_600_000,
      total: 24_000_000,
      notes: 'hidden',
      url: 'https://example.invalid/app.tar.gz',
    })
    channels[0]?.onmessage({ kind: 'installing', version: '0.1.4' })
    channels[0]?.onmessage({ kind: 'downloading', received: 'nope', total: 1 })

    expect(seen).toEqual([
      { kind: 'downloading', received: 9_600_000, total: 24_000_000 },
      { kind: 'installing' },
    ])
  })

  test('update_cancel returns true only when Rust says the download was aborted', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })

    invoke.mockResolvedValue(true)
    await expect(updateCancel()).resolves.toBe(true)
    expect(invoke).toHaveBeenCalledWith('update_cancel')

    invoke.mockResolvedValue(false)
    await expect(updateCancel()).resolves.toBe(false)

    invoke.mockResolvedValue({ kind: 'cancelled' })
    await expect(updateCancel()).resolves.toBe(false)

    invoke.mockResolvedValue(undefined)
    await expect(updateCancel()).resolves.toBe(false)
  })

  test('update_take_notice keeps from and to, and nothing else', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
    invoke.mockResolvedValue({ from: '0.1.3', to: '0.1.4', notes: 'nope' })
    await expect(updateTakeNotice()).resolves.toEqual({ from: '0.1.3', to: '0.1.4' })
    expect(invoke).toHaveBeenCalledWith('update_take_notice')

    invoke.mockResolvedValue(null)
    await expect(updateTakeNotice()).resolves.toBeNull()
  })

  test('update_install returns failed when Rust yields { kind: failed }', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
    invoke.mockResolvedValue({ kind: 'failed' })
    await expect(
      updateInstall({ kind: 'available', version: '0.1.1' }, () => undefined),
    ).resolves.toEqual({
      kind: 'failed',
    })
  })

  test('a failed check and a failed install keep the code Rust names', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })

    invoke.mockResolvedValue({ kind: 'failed', code: 'update_network' })
    await expect(updateCheck()).resolves.toEqual({ kind: 'failed', code: 'update_network' })

    invoke.mockResolvedValue({ kind: 'failed', code: 'update_install_failed' })
    await expect(
      updateInstall({ kind: 'available', version: '0.1.1' }, () => undefined),
    ).resolves.toEqual({
      kind: 'failed',
      code: 'update_install_failed',
    })

    invoke.mockResolvedValue({ kind: 'cancelled' })
    await expect(
      updateInstall({ kind: 'available', version: '0.1.1' }, () => undefined),
    ).resolves.toEqual({
      kind: 'cancelled',
    })
  })
})
