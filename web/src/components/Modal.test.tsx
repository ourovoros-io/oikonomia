/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, test } from 'vitest'

import { Modal } from './Modal'

afterEach(() => {
  cleanup()
})

describe('Modal', () => {
  test('portals its overlay to document.body, escaping any ancestor glass pane', () => {
    // backdrop-filter makes an element the containing block for its
    // position:fixed descendants in WebKit, so a Modal rendered inside
    // another glass surface would otherwise be sized to and clipped by it.
    render(
      <div data-testid="host">
        <Modal open title="Entry detail" onClose={() => {}}>
          content
        </Modal>
      </div>,
    )

    const dialog = screen.getByRole('dialog', { name: 'Entry detail' })
    expect(dialog.parentElement).toBe(document.body)
  })

  test('opens on its first field, not on the close button', () => {
    render(
      <Modal open title="New entry" onClose={() => {}}>
        <form>
          <input aria-label="Date" />
          <input aria-label="Amount" />
        </form>
      </Modal>,
    )

    expect(screen.getByLabelText('Date')).toHaveFocus()
  })

  test('opens on the panel when there is no text field to type into', () => {
    render(
      <Modal open title="Entry detail" onClose={() => {}}>
        <input type="checkbox" aria-label="Hide" />
      </Modal>,
    )

    expect(screen.getByLabelText('Hide')).not.toHaveFocus()
    expect(screen.getByRole('dialog').firstElementChild).toHaveFocus()
  })
})
