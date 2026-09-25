/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

import type { CashFlowSeries } from '../lib/api'
import { CashFlowPulse } from './CashFlowPulse'

const series: CashFlowSeries = {
  entity_id: 'e1',
  from: '2026-08-01',
  to: '2026-08-03',
  granularity: 'day',
  total_income_minor: 500,
  total_expenses_minor: 120,
  net_minor: 380,
  buckets: [
    {
      start: '2026-08-01',
      end: '2026-08-01',
      income_minor: 500,
      expenses_minor: 0,
      cumulative_income_minor: 500,
      cumulative_expenses_minor: 0,
    },
    {
      start: '2026-08-02',
      end: '2026-08-02',
      income_minor: 0,
      expenses_minor: 120,
      cumulative_income_minor: 500,
      cumulative_expenses_minor: 120,
    },
    {
      start: '2026-08-03',
      end: '2026-08-03',
      income_minor: 0,
      expenses_minor: 0,
      cumulative_income_minor: 500,
      cumulative_expenses_minor: 120,
    },
  ],
}

const formatAmount = (minor: number) => `€${(minor / 100).toFixed(2)}`

let height = 132

beforeEach(() => {
  height = 132
  vi.spyOn(Element.prototype, 'clientWidth', 'get').mockReturnValue(308)
  vi.spyOn(Element.prototype, 'clientHeight', 'get').mockImplementation(() => height)
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('CashFlowPulse', () => {
  test('is an image named by its text alternative', () => {
    render(
      <CashFlowPulse
        series={series}
        label="Money in €5.00, money out €1.20."
        formatAmount={formatAmount}
      />,
    )

    expect(
      screen.getByRole('img', { name: 'Money in €5.00, money out €1.20.' }),
    ).toBeInTheDocument()
  })

  test('draws one bar per side that moved money, and none for a quiet day', () => {
    const { container } = render(
      <CashFlowPulse series={series} label="pulse" formatAmount={formatAmount} />,
    )

    expect(container.querySelectorAll('[data-pulse-bar="in"]')).toHaveLength(1)
    expect(container.querySelectorAll('[data-pulse-bar="out"]')).toHaveLength(1)
  })

  test('labels the biggest day each way when there is room', () => {
    render(<CashFlowPulse series={series} label="pulse" formatAmount={formatAmount} />)

    const image = screen.getByRole('img')
    expect(image).toHaveTextContent('€5.00')
    expect(image).toHaveTextContent('€1.20')
  })

  test('sizes the glow region to the chart, never to the bars', () => {
    render(<CashFlowPulse series={series} label="pulse" formatAmount={formatAmount} />)

    const filter = screen.getByRole('img').querySelector('filter')
    expect(filter).toHaveAttribute('filterUnits', 'userSpaceOnUse')
    expect(filter).toHaveAttribute('width', '328')
    expect(filter).toHaveAttribute('height', '152')
  })

  test('leaves the peak labels out of a short strip', () => {
    height = 72
    render(<CashFlowPulse series={series} label="pulse" formatAmount={formatAmount} />)

    expect(screen.getByRole('img')).not.toHaveTextContent('€5.00')
  })

  test('reads the day under the pointer', () => {
    render(<CashFlowPulse series={series} label="pulse" formatAmount={formatAmount} />)
    const image = screen.getByRole('img')
    vi.spyOn(image, 'getBoundingClientRect').mockReturnValue({ left: 0, top: 0 } as DOMRect)

    // Bands are 100px wide after the 4px gutter; x = 160 is the second day.
    fireEvent.pointerMove(image, { clientX: 160 })

    const reading = screen.getByRole('status')
    expect(reading).toHaveTextContent(/Aug 2|2 Aug/)
    expect(reading).toHaveTextContent('€1.20')

    fireEvent.pointerLeave(image)
    expect(reading).toBeEmptyDOMElement()
  })

  test('walks the days with the arrow keys', () => {
    render(<CashFlowPulse series={series} label="pulse" formatAmount={formatAmount} />)
    const image = screen.getByRole('img')

    fireEvent.keyDown(image, { key: 'ArrowRight' })
    expect(screen.getByRole('status')).toHaveTextContent('€5.00')

    fireEvent.keyDown(image, { key: 'ArrowRight' })
    fireEvent.keyDown(image, { key: 'ArrowRight' })
    fireEvent.keyDown(image, { key: 'ArrowRight' })
    expect(screen.getByRole('status')).toHaveTextContent(/Aug 3|3 Aug/)

    fireEvent.keyDown(image, { key: 'Escape' })
    expect(screen.getByRole('status')).toBeEmptyDOMElement()
  })

  test('with no series it still names itself and draws no bars', () => {
    const { container } = render(
      <CashFlowPulse series={null} label="No cash flow to draw yet." formatAmount={formatAmount} />,
    )

    expect(screen.getByRole('img', { name: 'No cash flow to draw yet.' })).toBeInTheDocument()
    expect(container.querySelectorAll('[data-pulse-bar]')).toHaveLength(0)
    expect(screen.getByRole('img')).not.toHaveAttribute('tabindex')
  })
})
