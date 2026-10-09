/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { DateInput } from './DateInput'
import { Modal } from './Modal'

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

  test('the calendar of a field in a dialog is drawn in that dialog, not behind it', async () => {
    render(
      <Modal open title="Set balance" onClose={vi.fn()}>
        <DateInput value="2026-10-08" onChange={vi.fn()} aria-label="Date" />
      </Modal>,
    )

    await userEvent.click(screen.getByRole('button', { name: 'Open calendar' }))

    const dialog = screen.getByRole('dialog', { name: 'Set balance' })
    expect(dialog).toContainElement(screen.getByRole('dialog', { name: 'Calendar' }))
  })

  test('the calendar is placed in the window from where the field is', async () => {
    render(<DateInput value="2026-10-08" onChange={vi.fn()} aria-label="Date" />)
    const field = screen.getByLabelText('Date').closest('div')?.parentElement as HTMLElement
    vi.spyOn(field, 'getBoundingClientRect').mockReturnValue(new DOMRect(300, 100, 150, 40))

    await userEvent.click(screen.getByRole('button', { name: 'Open calendar' }))

    const calendar = screen.getByRole('dialog', { name: 'Calendar' })
    expect(calendar.style.top).toBe('148px')
    expect(calendar.style.left).toBe('300px')
  })

  test('every month is drawn six weeks tall, so paging months moves nothing', async () => {
    render(<DateInput value="2026-10-08" onChange={vi.fn()} aria-label="Date" />)
    await userEvent.click(screen.getByRole('button', { name: 'Open calendar' }))
    const grid = screen.getByRole('button', { name: '15' }).parentElement as HTMLElement
    const cells = () => grid.children.length

    const october = cells()
    await userEvent.click(screen.getByRole('button', { name: 'Next month' }))
    expect(screen.getByText('November 2026')).toBeInTheDocument()
    expect(cells()).toBe(october)
    // February 2027 starts on a Monday: four weeks, the shortest a month gets.
    for (let i = 0; i < 3; i++)
      await userEvent.click(screen.getByRole('button', { name: 'Next month' }))
    expect(screen.getByText('February 2027')).toBeInTheDocument()
    expect(cells()).toBe(october)
  })

  test('Tab reaches the calendar from its button and stays in it; Escape hands focus back', async () => {
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

    await userEvent.tab()
    expect(screen.getByRole('button', { name: 'Next month' })).toHaveFocus()

    await userEvent.tab({ shift: true })
    await userEvent.tab({ shift: true })
    expect(screen.getByRole('button', { name: '31' })).toHaveFocus()

    await userEvent.keyboard('{Escape}')
    expect(screen.queryByRole('dialog', { name: 'Calendar' })).toBeNull()
    expect(toggle).toHaveFocus()
    expect(onClose).not.toHaveBeenCalled()
  })

  test('Escape and Tab reach the calendar when nothing has focus, as after a click in WebKit', async () => {
    const onClose = vi.fn()
    render(
      <Modal open title="Set balance" onClose={onClose}>
        <DateInput value="2026-10-08" onChange={vi.fn()} aria-label="Date" />
      </Modal>,
    )
    await userEvent.click(screen.getByRole('button', { name: 'Open calendar' }))
    ;(document.activeElement as HTMLElement).blur()
    expect(document.body).toHaveFocus()

    await userEvent.tab()
    expect(screen.getByRole('button', { name: 'Previous month' })).toHaveFocus()

    ;(document.activeElement as HTMLElement).blur()
    await userEvent.keyboard('{Escape}')
    expect(screen.queryByRole('dialog', { name: 'Calendar' })).toBeNull()
    expect(onClose).not.toHaveBeenCalled()
  })

  test('Tab from the date itself goes to the calendar button, not into the calendar', async () => {
    render(<DateInput value="2026-10-08" onChange={vi.fn()} aria-label="Date" />)
    const toggle = screen.getByRole('button', { name: 'Open calendar' })
    await userEvent.click(toggle)

    screen.getByLabelText('Date').focus()
    await userEvent.tab()

    expect(toggle).toHaveFocus()
  })

  test('typing a date closes the calendar', async () => {
    render(<DateInput value="2026-10-08" onChange={vi.fn()} aria-label="Date" />)
    await userEvent.click(screen.getByRole('button', { name: 'Open calendar' }))

    await userEvent.type(screen.getByLabelText('Date'), '1')

    expect(screen.queryByRole('dialog', { name: 'Calendar' })).toBeNull()
  })

  test('the calendar closes when the page scrolls or the window resizes, and only then', async () => {
    render(
      <>
        <div data-testid="other-pane" />
        <DateInput value="2026-10-08" onChange={vi.fn()} aria-label="Date" />
      </>,
    )
    const toggle = screen.getByRole('button', { name: 'Open calendar' })
    const calendar = () => screen.queryByRole('dialog', { name: 'Calendar' })

    await userEvent.click(toggle)
    fireEvent.scroll(screen.getByTestId('other-pane'))
    expect(calendar()).not.toBeNull()

    fireEvent.scroll(document)
    expect(calendar()).toBeNull()

    await userEvent.click(toggle)
    fireEvent(window, new Event('resize'))
    expect(calendar()).toBeNull()
  })
})
