import { describe, expect, test } from 'vitest'

import { quarterEndISO, quarterStartISO } from './api'

describe('calendar quarter bounds', () => {
  test('mid-quarter', () => {
    const now = new Date(2026, 7, 15)
    expect([quarterStartISO(now), quarterEndISO(now)]).toEqual(['2026-07-01', '2026-09-30'])
  })

  test('the first and last days of the year', () => {
    expect([quarterStartISO(new Date(2026, 0, 1)), quarterEndISO(new Date(2026, 0, 1))]).toEqual([
      '2026-01-01',
      '2026-03-31',
    ])
    expect([quarterStartISO(new Date(2026, 11, 31)), quarterEndISO(new Date(2026, 11, 31))]).toEqual([
      '2026-10-01',
      '2026-12-31',
    ])
  })
})
