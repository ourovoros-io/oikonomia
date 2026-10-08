/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

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
})

describe('PageErrorBoundary', () => {
  test('shows a message instead of a blank window, and retry recovers', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    render(
      <PageErrorBoundary resetKey="reports:e1">
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
      <PageErrorBoundary resetKey="reports:e1">
        <Bomb />
      </PageErrorBoundary>,
    )
    shouldThrow = false

    rerender(
      <PageErrorBoundary resetKey="reports:e2">
        <Bomb />
      </PageErrorBoundary>,
    )

    expect(screen.getByText('page content')).toBeInTheDocument()
  })
})
