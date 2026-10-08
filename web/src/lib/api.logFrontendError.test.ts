/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

import { invoke } from '@tauri-apps/api/core'
import { api } from './api'

function setTauri(present: boolean) {
  if (present) {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
  } else {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  }
}

beforeEach(() => {
  vi.mocked(invoke).mockReset()
  vi.mocked(invoke).mockResolvedValue(undefined)
})

afterEach(() => setTauri(false))

describe('api.logFrontendError', () => {
  test('sends the location, the error name and the message, and nothing else', () => {
    setTauri(true)
    const error = new TypeError('boom')

    api.logFrontendError('reports', error)

    expect(invoke).toHaveBeenCalledWith('log_frontend_error', {
      location: 'reports',
      name: 'TypeError',
      message: 'boom',
    })
  })

  test('a value that is not an Error sends no text of its own', () => {
    setTauri(true)

    api.logFrontendError('window', 'Rent 1200 EUR')

    expect(invoke).toHaveBeenCalledWith('log_frontend_error', {
      location: 'window',
      name: 'NonError',
      message: '',
    })
  })

  test('does nothing outside the desktop app', () => {
    setTauri(false)

    api.logFrontendError('dashboard', new Error('x'))

    expect(invoke).not.toHaveBeenCalled()
  })

  test('never throws, whether invoke rejects or throws', async () => {
    setTauri(true)
    vi.mocked(invoke).mockRejectedValueOnce(new Error('ipc down'))
    expect(() => api.logFrontendError('window', new Error('x'))).not.toThrow()

    vi.mocked(invoke).mockImplementationOnce(() => {
      throw new Error('sync')
    })
    expect(() => api.logFrontendError('window', new Error('x'))).not.toThrow()

    await Promise.resolve()
  })
})
