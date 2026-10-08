/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { DateInput } from './DateInput'

afterEach(() => {
  cleanup()
})

describe('DateInput while typing', () => {
  test('reports a complete date before the field is left', async () => {
    const onChange = vi.fn()
    render(<DateInput value="" onChange={onChange} aria-label="From" />)

    await userEvent.type(screen.getByLabelText('From'), '30/09/2026')

    expect(onChange).toHaveBeenLastCalledWith('2026-09-30')
  })

  test('waits for the whole year, so 01/10/20 is not taken as 2020', async () => {
    const onChange = vi.fn()
    render(<DateInput value="" onChange={onChange} aria-label="From" />)

    await userEvent.type(screen.getByLabelText('From'), '01/10/20')

    expect(onChange).not.toHaveBeenCalled()
  })
})
