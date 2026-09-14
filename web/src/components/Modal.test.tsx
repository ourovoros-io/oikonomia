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
})
