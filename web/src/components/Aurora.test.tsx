/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { Aurora } from './Aurora'

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('Aurora', () => {
  test('is decorative and hidden from assistive technology', () => {
    const { container } = render(<Aurora />)

    expect(container.firstElementChild).toHaveAttribute('aria-hidden', 'true')
  })

  test('drifts while the window has focus', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)

    const { container } = render(<Aurora />)

    expect(container.firstElementChild).toHaveAttribute('data-moving', 'true')
  })

  test('holds still in a background window', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(false)

    const { container } = render(<Aurora />)

    expect(container.firstElementChild).toHaveAttribute('data-moving', 'false')
  })

  test('shows the Greek wordmark unless asked not to', () => {
    const { container, rerender } = render(<Aurora />)
    expect(container).toHaveTextContent('ΟΙΚΟΝΟΜΙΑ')

    rerender(<Aurora watermark={false} />)
    expect(container).not.toHaveTextContent('ΟΙΚΟΝΟΜΙΑ')
  })
})
