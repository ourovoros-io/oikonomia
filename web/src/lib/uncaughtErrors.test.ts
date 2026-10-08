/** @vitest-environment jsdom */

import { afterEach, describe, expect, test, vi } from 'vitest'

vi.mock('./api', () => ({
  api: { logFrontendError: vi.fn() },
}))

import { api } from './api'
import { reportUncaughtErrors } from './uncaughtErrors'

// A bare EventTarget: dispatching an ErrorEvent on the jsdom window would also
// be reported by jsdom as an uncaught error and fail the run.
function fakeWindow(): Window {
  return new EventTarget() as Window
}

afterEach(() => vi.mocked(api.logFrontendError).mockClear())

describe('reportUncaughtErrors', () => {
  test('logs an uncaught error and an unhandled rejection against the window', () => {
    const target = fakeWindow()
    const stop = reportUncaughtErrors(target)
    const error = new RangeError('bad')

    target.dispatchEvent(new ErrorEvent('error', { error }))
    const rejection = new Event('unhandledrejection') as PromiseRejectionEvent
    Object.defineProperty(rejection, 'reason', { value: 'why' })
    target.dispatchEvent(rejection)

    expect(api.logFrontendError).toHaveBeenNthCalledWith(1, 'window', error)
    expect(api.logFrontendError).toHaveBeenNthCalledWith(2, 'window', 'why')
    stop()
  })

  test('the returned function removes the listeners', () => {
    const target = fakeWindow()

    reportUncaughtErrors(target)()
    target.dispatchEvent(new ErrorEvent('error', { error: new Error('x') }))

    expect(api.logFrontendError).not.toHaveBeenCalled()
  })
})
