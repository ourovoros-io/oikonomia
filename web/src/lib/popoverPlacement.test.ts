import { describe, expect, test } from 'vitest'

import { placePopover } from './popoverPlacement'

const calendar = { width: 256, height: 258 }
const window960 = { width: 960, height: 640 }

describe('placePopover', () => {
  test('hangs below the field, left edges aligned, when there is room', () => {
    const place = placePopover({ top: 100, bottom: 140, left: 300 }, calendar, window960)

    expect(place).toEqual({ top: 148, left: 300 })
  })

  test('goes above a field that sits too low for it', () => {
    const place = placePopover({ top: 500, bottom: 540, left: 300 }, calendar, window960)

    expect(place).toEqual({ top: 500 - 8 - 258, left: 300 })
  })

  test('still fits below when it ends exactly at the window margin', () => {
    const bottom = 640 - 8 - 258 - 8
    const place = placePopover({ top: bottom - 40, bottom, left: 300 }, calendar, window960)

    expect(place.top).toBe(bottom + 8)
  })

  test('stays inside the window when it fits on neither side', () => {
    const short = { width: 960, height: 400 }
    const place = placePopover({ top: 180, bottom: 220, left: 300 }, calendar, short)

    expect(place.top).toBe(400 - 8 - 258)
  })

  test('is pulled back from the right edge of the window', () => {
    const place = placePopover({ top: 100, bottom: 140, left: 900 }, calendar, window960)

    expect(place.left).toBe(960 - 8 - 256)
  })

  test('is pulled back from the left edge of the window', () => {
    const place = placePopover({ top: 100, bottom: 140, left: -40 }, calendar, window960)

    expect(place.left).toBe(8)
  })

  test('keeps its top-left corner on screen in a window smaller than itself', () => {
    const tiny = { width: 200, height: 200 }
    const place = placePopover({ top: 20, bottom: 60, left: 40 }, calendar, tiny)

    expect(place).toEqual({ top: 8, left: 8 })
  })
})
