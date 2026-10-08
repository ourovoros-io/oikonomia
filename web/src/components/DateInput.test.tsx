/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { DateInput } from './DateInput'

afterEach(cleanup)

describe('DateInput', () => {
  test('an impossible date clears the value and says why, instead of keeping the old date', async () => {
    const onChange = vi.fn()
    render(<DateInput value="2026-10-08" onChange={onChange} aria-label="Date" />)

    const input = screen.getByLabelText('Date')
    await userEvent.clear(input)
    await userEvent.type(input, '31/02/2026')
    await userEvent.tab()

    expect(onChange).toHaveBeenLastCalledWith('')
    expect(screen.getByRole('alert')).toHaveTextContent(/real date/)
  })

  test('a date outside the supported years is refused', async () => {
    const onChange = vi.fn()
    render(<DateInput value="2026-10-08" onChange={onChange} aria-label="Date" />)

    const input = screen.getByLabelText('Date')
    await userEvent.clear(input)
    await userEvent.type(input, '01/01/1890')
    await userEvent.tab()

    expect(onChange).toHaveBeenLastCalledWith('')
  })

  test('a valid date commits and shows no message', async () => {
    const onChange = vi.fn()
    render(<DateInput value="2026-10-08" onChange={onChange} aria-label="Date" />)

    const input = screen.getByLabelText('Date')
    await userEvent.clear(input)
    await userEvent.type(input, '25/12/2026')
    await userEvent.tab()

    expect(onChange).toHaveBeenLastCalledWith('2026-12-25')
    expect(screen.queryByRole('alert')).toBeNull()
  })
})
