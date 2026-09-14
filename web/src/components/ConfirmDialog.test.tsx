/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, test } from 'vitest'

import { ConfirmDialog } from './ConfirmDialog'

afterEach(() => {
  cleanup()
})

describe('ConfirmDialog', () => {
  test('portals to document.body even when opened inside another glass dialog', () => {
    // The failure case this guards: ConfirmDialog nested inside Modal (e.g.
    // EntryDetailModal's delete confirm) would otherwise be sized to and
    // clipped by the outer Modal's backdrop-filter containing block.
    render(
      <div data-testid="outer-dialog">
        <ConfirmDialog
          open
          title="Delete document?"
          body="This cannot be undone."
          onConfirm={() => {}}
          onCancel={() => {}}
        />
      </div>,
    )

    const dialog = screen.getByRole('dialog', { name: 'Delete document?' })
    expect(dialog.parentElement).toBe(document.body)
  })
})
