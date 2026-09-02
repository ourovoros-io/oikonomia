/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, test } from 'vitest'

import { ErrorBanner } from './ui'

afterEach(() => {
  cleanup()
})

describe('ErrorBanner', () => {
  test('renders nothing when there is no message or title', () => {
    render(<ErrorBanner message={null} />)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('announces the message via role="alert" and aria-live="assertive"', () => {
    render(<ErrorBanner message="Something went wrong." />)
    const banner = screen.getByRole('alert')
    expect(banner).toHaveAttribute('aria-live', 'assertive')
    expect(banner).toHaveTextContent('Something went wrong.')
  })

  test('accepts an id so callers can wire aria-describedby to it', () => {
    render(<ErrorBanner id="form-error" message="Bad value." />)
    expect(screen.getByRole('alert')).toHaveAttribute('id', 'form-error')
  })
})
