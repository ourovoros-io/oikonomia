/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render } from '@testing-library/react'
import { afterEach, describe, expect, test } from 'vitest'

import { Aurora } from './Aurora'

afterEach(() => {
  cleanup()
})

describe('Aurora', () => {
  test('is decorative and hidden from assistive technology', () => {
    const { container } = render(<Aurora />)

    expect(container.firstElementChild).toHaveAttribute('aria-hidden', 'true')
  })

  test('does not paint the giant wordmark', () => {
    const { container } = render(<Aurora />)
    expect(container).not.toHaveTextContent('ΟΙΚΟΝΟΜΙΑ')
  })
})
