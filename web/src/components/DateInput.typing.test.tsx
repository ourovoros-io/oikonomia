/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { applyLocale, resetI18nForTests, type Locale } from '../lib/i18n'
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

describe('DateInput in each language', () => {
  afterEach(() => resetI18nForTests())

  test.each<[Locale, string, string]>([
    ['en', '16/10/2026', 'dd/mm/yyyy'],
    ['el', '16/10/2026', 'ηη/μμ/εεεε'],
    ['fr', '16/10/2026', 'jj/mm/aaaa'],
    ['de', '16.10.2026', 'TT.MM.JJJJ'],
  ])('%s shows %s with the hint %s', (locale, shown, hint) => {
    applyLocale(locale)
    render(<DateInput value="2026-10-16" onChange={vi.fn()} aria-label="Date" />)

    const input = screen.getByLabelText('Date')
    expect(input).toHaveValue(shown)
    expect(input).toHaveAttribute('placeholder', hint)
  })

  test.each<[Locale, string]>([
    ['en', '16/10/2026'],
    ['el', '16/10/2026'],
    ['fr', '16/10/2026'],
    ['de', '16.10.2026'],
  ])('%s reads slashes and dots and shows its own separator', async (locale, shown) => {
    applyLocale(locale)

    for (const typed of ['16/10/2026', '16.10.2026', '16.10.26']) {
      const onChange = vi.fn()
      render(<DateInput value="" onChange={onChange} aria-label="Date" />)
      const input = screen.getByLabelText('Date')

      await userEvent.type(input, typed)
      await userEvent.tab()

      expect(onChange).toHaveBeenLastCalledWith('2026-10-16')
      expect(input).toHaveValue(shown)
      cleanup()
    }
  })

  test('a language change rewrites the date with the new separator', () => {
    applyLocale('en')
    render(<DateInput value="2026-10-16" onChange={vi.fn()} aria-label="Date" />)

    act(() => applyLocale('de'))

    expect(screen.getByLabelText('Date')).toHaveValue('16.10.2026')
  })
})
