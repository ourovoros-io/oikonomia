/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test } from 'vitest'

import { trapTab } from './useDialogFocus'

afterEach(() => {
  cleanup()
})

function Trapped() {
  return (
    <div onKeyDown={(e) => trapTab(e.nativeEvent, e.currentTarget)}>
      <button>first</button>
      <button>last</button>
    </div>
  )
}

// jsdom has no layout, so offsetParent is always null; give controls one.
function withLayout() {
  Object.defineProperty(HTMLElement.prototype, 'offsetParent', {
    configurable: true,
    get() {
      return this.parentElement
    },
  })
}

describe('trapTab', () => {
  test('Tab from the last control wraps to the first, and Shift+Tab goes back', async () => {
    withLayout()
    render(<Trapped />)
    const first = screen.getByRole('button', { name: 'first' })
    const last = screen.getByRole('button', { name: 'last' })

    last.focus()
    await userEvent.tab()
    expect(first).toHaveFocus()

    fireEvent.keyDown(first, { key: 'Tab', shiftKey: true })
    expect(last).toHaveFocus()
  })
})
