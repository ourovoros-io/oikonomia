/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { DateInput } from './DateInput'
import { Modal } from './Modal'

afterEach(cleanup)

// jsdom has no layout, so offsetParent is always null; give controls one.
function withLayout() {
  Object.defineProperty(HTMLElement.prototype, 'offsetParent', {
    configurable: true,
    get() {
      return this.parentElement
    },
  })
}

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

  test('the calendar is drawn outside the field, so no pane or dialog can clip it', async () => {
    const { container } = render(
      <DateInput value="2026-10-08" onChange={vi.fn()} aria-label="Date" />,
    )

    await userEvent.click(screen.getByRole('button', { name: 'Open calendar' }))

    const calendar = screen.getByText('October 2026')
    expect(container).not.toContainElement(calendar)
  })

  test('picking a day in a dialog sets the date and leaves the dialog open', async () => {
    const onChange = vi.fn()
    const onClose = vi.fn()
    render(
      <Modal open title="Set balance" onClose={onClose}>
        <DateInput value="2026-10-08" onChange={onChange} aria-label="Date" />
      </Modal>,
    )

    await userEvent.click(screen.getByRole('button', { name: 'Open calendar' }))
    await userEvent.click(screen.getByRole('button', { name: '21' }))

    expect(onChange).toHaveBeenLastCalledWith('2026-10-21')
    expect(onClose).not.toHaveBeenCalled()
    expect(screen.queryByText('October 2026')).toBeNull()
  })

  test('Tab reaches the calendar from its button and stays in it; Escape hands focus back', async () => {
    withLayout()
    const onClose = vi.fn()
    render(
      <Modal open title="Set balance" onClose={onClose}>
        <DateInput value="2026-10-08" onChange={vi.fn()} aria-label="Date" />
      </Modal>,
    )
    const toggle = screen.getByRole('button', { name: 'Open calendar' })

    await userEvent.click(toggle)
    await userEvent.tab()
    expect(screen.getByRole('button', { name: 'Previous month' })).toHaveFocus()

    await userEvent.tab({ shift: true })
    expect(screen.getByRole('button', { name: '31' })).toHaveFocus()

    await userEvent.keyboard('{Escape}')
    expect(screen.queryByText('October 2026')).toBeNull()
    expect(toggle).toHaveFocus()
    expect(onClose).not.toHaveBeenCalled()
  })

  test('scrolling the page closes the calendar, which would otherwise be left behind', async () => {
    render(<DateInput value="2026-10-08" onChange={vi.fn()} aria-label="Date" />)
    await userEvent.click(screen.getByRole('button', { name: 'Open calendar' }))

    fireEvent.scroll(document)

    expect(screen.queryByText('October 2026')).toBeNull()
  })
})
