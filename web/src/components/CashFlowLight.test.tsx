/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

import type { CashFlowSeries } from '../lib/api'
import { CashFlowLight } from './CashFlowLight'

const series: CashFlowSeries = {
  entity_id: 'e1',
  from: '2026-08-01',
  to: '2026-08-02',
  granularity: 'day',
  total_income_minor: 500,
  total_expenses_minor: 120,
  net_minor: 380,
  buckets: [
    { start: '2026-08-01', end: '2026-08-01', income_minor: 500, expenses_minor: 0, cumulative_income_minor: 500, cumulative_expenses_minor: 0 },
    { start: '2026-08-02', end: '2026-08-02', income_minor: 0, expenses_minor: 120, cumulative_income_minor: 500, cumulative_expenses_minor: 120 },
  ],
}

function fakeContext() {
  const gradient = { addColorStop: vi.fn() }
  return {
    setTransform: vi.fn(),
    clearRect: vi.fn(),
    createLinearGradient: vi.fn(() => gradient),
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    closePath: vi.fn(),
    fill: vi.fn(),
    stroke: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 1,
    shadowBlur: 0,
    shadowColor: '',
    globalCompositeOperation: 'source-over',
  }
}

let context: ReturnType<typeof fakeContext> | null

beforeEach(() => {
  context = fakeContext()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation((() => context) as never)
  vi.spyOn(Element.prototype, 'clientWidth', 'get').mockReturnValue(600)
  vi.spyOn(Element.prototype, 'clientHeight', 'get').mockReturnValue(170)
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('CashFlowLight', () => {
  test('is an image named by its text alternative; the canvases stay silent', () => {
    render(<CashFlowLight series={series} label="Money in 5,00 €, money out 1,20 €, net +3,80 €, August 2026." className="h-[170px]" />)

    const light = screen.getByRole('img', { name: 'Money in 5,00 €, money out 1,20 €, net +3,80 €, August 2026.' })
    const canvases = light.querySelectorAll('canvas')
    expect(canvases).toHaveLength(2)
    canvases.forEach((canvas) => expect(canvas).toHaveAttribute('aria-hidden', 'true'))
  })

  test('paints both bands once it has a box to paint into', () => {
    render(<CashFlowLight series={series} label="light" />)

    expect(context?.fill).toHaveBeenCalled()
    expect(context?.stroke).toHaveBeenCalled()
    expect(context?.createLinearGradient).toHaveBeenCalled()
  })

  test('holds still when the window is not focused', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(false)
    const frame = vi.spyOn(window, 'requestAnimationFrame').mockImplementation(() => 1)

    render(<CashFlowLight series={series} label="light" />)

    expect(frame).not.toHaveBeenCalled()
  })

  test('breathes while the window is visible and focused', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    const frame = vi.spyOn(window, 'requestAnimationFrame').mockImplementation(() => 1)
    vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => undefined)

    render(<CashFlowLight series={series} label="light" />)

    expect(frame).toHaveBeenCalled()
  })

  test('survives a webview with no 2D canvas', () => {
    context = null

    expect(() => render(<CashFlowLight series={series} label="light" />)).not.toThrow()
    expect(screen.getByRole('img', { name: 'light' })).toBeInTheDocument()
  })
})
