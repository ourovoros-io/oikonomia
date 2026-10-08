/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', () => ({
  api: { logFrontendError: vi.fn() },
}))

import { api } from '../lib/api'
import { PageErrorBoundary } from './PageErrorBoundary'

let shouldThrow = true

function Bomb() {
  if (shouldThrow) throw new Error('boom')
  return <p>page content</p>
}

afterEach(() => {
  cleanup()
  shouldThrow = true
  vi.restoreAllMocks()
  vi.mocked(api.logFrontendError).mockClear()
})

describe('PageErrorBoundary', () => {
  test('shows a message instead of a blank window, and retry recovers', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    render(
      <PageErrorBoundary page="reports" resetKey="reports:e1">
        <Bomb />
      </PageErrorBoundary>,
    )

    expect(screen.getByText(/could not be shown/)).toBeInTheDocument()

    shouldThrow = false
    await userEvent.click(screen.getByRole('button', { name: 'Try again' }))

    expect(screen.getByText('page content')).toBeInTheDocument()
  })

  test('a new page or book clears the error', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    const { rerender } = render(
      <PageErrorBoundary page="reports" resetKey="reports:e1">
        <Bomb />
      </PageErrorBoundary>,
    )
    shouldThrow = false

    rerender(
      <PageErrorBoundary page="reports" resetKey="reports:e2">
        <Bomb />
      </PageErrorBoundary>,
    )

    expect(screen.getByText('page content')).toBeInTheDocument()
  })

  test('logs the failing page and the error, and still writes to the console', () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {})
    render(
      <PageErrorBoundary page="reports" resetKey="reports:e1">
        <Bomb />
      </PageErrorBoundary>,
    )

    expect(api.logFrontendError).toHaveBeenCalledTimes(1)
    expect(api.logFrontendError).toHaveBeenCalledWith(
      'reports',
      expect.objectContaining({ message: 'boom' }),
    )
    expect(consoleError).toHaveBeenCalled()
  })
})
