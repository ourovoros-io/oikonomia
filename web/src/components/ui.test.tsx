/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, test } from 'vitest'

import { Button, ErrorBanner, Field, IconBadge, Input, MetricCard, Select } from './ui'

afterEach(() => {
  cleanup()
})

describe('ErrorBanner', () => {
  test('renders nothing when there is no message or title', () => {
    render(<ErrorBanner message={null} />)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('announces the message via role="alert" and aria-live="assertive"', () => {
    render(<ErrorBanner message="Something went wrong." />)
    const banner = screen.getByRole('alert')
    expect(banner).toHaveAttribute('aria-live', 'assertive')
    expect(banner).toHaveTextContent('Something went wrong.')
  })

  test('accepts an id so callers can wire aria-describedby to it', () => {
    render(<ErrorBanner id="form-error" message="Bad value." />)
    expect(screen.getByRole('alert')).toHaveAttribute('id', 'form-error')
  })
})

describe('Field', () => {
  test('still labels the control it wraps', () => {
    render(
      <Field label="Amount">
        <Input defaultValue="25,50" />
      </Field>,
    )

    expect(screen.getByLabelText('Amount')).toHaveValue('25,50')
  })

  test('carries the hooks the glass field box is styled by', () => {
    render(
      <Field label="Account">
        <Select defaultValue="bank" aria-label="Account">
          <option value="bank">Bank</option>
        </Select>
      </Field>,
    )

    // index.css draws one glass box for .field-box:has(.ui-control) and strips
    // the control's own box. Renaming either class silently breaks every form.
    // (Selects are found by aria-label, as in the app: option text would leak
    // into a wrapping label's text.)
    const control = screen.getByRole('combobox', { name: 'Account' })
    expect(control).toHaveClass('ui-control')
    expect(control.closest('label')).toHaveClass('field-box')
  })
})

describe('IconBadge', () => {
  test('money-in and money-out tones wear the Ledger colours, never status', () => {
    const { rerender } = render(<IconBadge tone="money-in">in</IconBadge>)
    expect(screen.getByText('in')).toHaveClass('bg-[var(--color-money-in-soft)]')
    expect(screen.getByText('in')).toHaveClass('text-[var(--color-money-in-text)]')

    rerender(<IconBadge tone="money-out">out</IconBadge>)
    expect(screen.getByText('out')).toHaveClass('bg-[var(--color-money-out-soft)]')
    expect(screen.getByText('out')).toHaveClass('text-[var(--color-money-out-text)]')
  })
})

describe('MetricCard', () => {
  test('success/danger accent icons wear the Ledger money tones, not status colours', () => {
    // Money identity always uses the Ledger; status colours (success/danger)
    // are for confirmations and errors, never for money in/out.
    const { rerender } = render(
      <MetricCard label="Income" value="100" icon={<span data-testid="icon" />} accent="success" />,
    )
    expect(screen.getByTestId('icon').parentElement).toHaveClass('bg-[var(--color-money-in-soft)]')

    rerender(
      <MetricCard label="Expenses" value="50" icon={<span data-testid="icon" />} accent="danger" />,
    )
    expect(screen.getByTestId('icon').parentElement).toHaveClass('bg-[var(--color-money-out-soft)]')
  })
})

describe('Button', () => {
  test('a busy button keeps its name and cannot be pressed twice', () => {
    render(<Button busy>Save entry</Button>)

    expect(screen.getByRole('button', { name: 'Save entry' })).toBeDisabled()
  })

  test('exposes its variant through data-variant attribute', () => {
    render(<Button variant="ghost">Post</Button>)
    expect(screen.getByRole('button', { name: 'Post' })).toHaveAttribute('data-variant', 'ghost')
  })

  test('defaults to primary variant through data-variant attribute', () => {
    render(<Button>Save entry</Button>)
    expect(screen.getByRole('button', { name: 'Save entry' })).toHaveAttribute('data-variant', 'primary')
  })
})
