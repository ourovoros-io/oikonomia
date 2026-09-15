/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, test } from 'vitest'

import { ArcTile } from './Arc'

afterEach(() => {
  cleanup()
})

describe('ArcTile', () => {
  test('is a meter carrying its value, label and hint', () => {
    render(<ArcTile label="Savings rate" hint="Net ÷ income" bps={6900} tone="in" locale="en" noValueLabel="No value yet" />)

    const meter = screen.getByRole('meter', { name: 'Savings rate' })
    expect(meter).toHaveAttribute('aria-valuenow', '69')
    expect(meter).toHaveAttribute('aria-valuetext', '69.0%')
    expect(meter).toHaveAccessibleDescription('Net ÷ income')
    expect(meter.querySelector('[data-arc-value]')).not.toBeNull()
    expect(meter.querySelector('svg')).toHaveAttribute('data-arc-tone', 'in')
  })

  test('a missing value is an empty arc with an em dash, never 0%', () => {
    render(<ArcTile label="Net vs last period" bps={null} tone="in" locale="en" noValueLabel="No value yet" />)

    const meter = screen.getByRole('meter', { name: 'Net vs last period' })
    expect(meter).toHaveAttribute('aria-valuenow', '0')
    expect(meter).toHaveAttribute('aria-valuetext', 'No value yet')
    expect(meter).toHaveTextContent('—')
    expect(meter).not.toHaveTextContent('0')
    expect(meter.querySelector('[data-arc-value]')).toBeNull()
  })

  test('a signed change lights the arc by its size and keeps its sign in the text', () => {
    render(<ArcTile label="Net vs last period" bps={-2000} signed tone="out" locale="en" noValueLabel="No value yet" />)

    const meter = screen.getByRole('meter', { name: 'Net vs last period' })
    expect(meter).toHaveAttribute('aria-valuenow', '20')
    expect(meter).toHaveAttribute('aria-valuetext', '-20.0%')
    expect(meter.querySelector('svg')).toHaveAttribute('data-arc-tone', 'out')
  })

  test('values past full scale fill the arc and no further', () => {
    render(<ArcTile label="Spend ratio" bps={25000} tone="out" locale="en" noValueLabel="No value yet" />)

    expect(screen.getByRole('meter', { name: 'Spend ratio' })).toHaveAttribute('aria-valuenow', '100')
  })

  test('a long label and hint stay reachable through title when clamped to two lines', () => {
    const label = 'Ποσοστό αποταμίευσης, πόσο κρατήσατε από όσα μπήκαν'
    const hint = 'Καθαρό έναντι του προηγούμενου μήνα, σε ποσοστό επί των εσόδων'
    render(<ArcTile label={label} hint={hint} bps={4200} tone="in" locale="en" noValueLabel="No value yet" />)

    expect(screen.getByTitle(label)).toBeTruthy()
    expect(screen.getByTitle(hint)).toBeTruthy()
  })
})
