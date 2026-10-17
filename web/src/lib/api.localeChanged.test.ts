/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

import { listen, type EventCallback } from '@tauri-apps/api/event'
import { api } from './api'

function setTauri(present: boolean) {
  if (present) {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true })
  } else {
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  }
}

/** The callback the wrapper registered, and a way to finish registering it. */
let registered: EventCallback<unknown> = () => undefined
let finishListening: (unlisten: () => void) => void = () => undefined

beforeEach(() => {
  vi.mocked(listen)
    .mockReset()
    .mockImplementation((_event, handler) => {
      registered = handler as EventCallback<unknown>
      return new Promise((resolve) => {
        finishListening = resolve
      })
    })
})

afterEach(() => setTauri(false))

describe('api.onLocaleChanged', () => {
  test('passes on the language Rust sends with locale-changed', () => {
    setTauri(true)
    const apply = vi.fn()

    api.onLocaleChanged(apply)
    registered({ event: 'locale-changed', id: 1, payload: 'el' })

    expect(listen).toHaveBeenCalledWith('locale-changed', expect.any(Function))
    expect(apply).toHaveBeenCalledWith('el')
  })

  test('stopping removes the listener, even before it was in place', async () => {
    setTauri(true)
    const unlisten = vi.fn()

    const stop = api.onLocaleChanged(vi.fn())
    stop()
    finishListening(unlisten)
    await Promise.resolve()

    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  test('outside the desktop app it listens for nothing', () => {
    const stop = api.onLocaleChanged(vi.fn())
    stop()

    expect(listen).not.toHaveBeenCalled()
  })
})
