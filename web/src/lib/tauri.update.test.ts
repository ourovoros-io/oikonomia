/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

const invoke = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}))

import { updateCheck, updateInstall } from './tauri'

afterEach(() => {
  invoke.mockReset()
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
    await updateInstall({ kind: 'available', version: '0.1.1' })
    expect(invoke).toHaveBeenCalledWith('update_install')
  })

  test('update_install returns failed when Rust yields { kind: failed }', async () => {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
    invoke.mockResolvedValue({ kind: 'failed' })
    await expect(updateInstall({ kind: 'available', version: '0.1.1' })).resolves.toEqual({
      kind: 'failed',
    })
  })
})
