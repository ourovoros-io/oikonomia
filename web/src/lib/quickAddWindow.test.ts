import { describe, expect, test } from 'vitest'
import {
  QUICK_ADD_COMPACT_HEIGHT,
  QUICK_ADD_SAVE_HEIGHT,
  QUICK_ADD_STEPPER_HEIGHT,
  QUICK_ADD_WIDTH,
} from './quickAddWindow'

/**
 * Keep these in sync with `QUICK_ADD_*` in
 * `apps/desktop/src-tauri/src/tray.rs` (Rust default create size must match
 * QUICK_ADD_STEPPER_HEIGHT / QUICK_ADD_WIDTH).
 */
describe('quickAddWindow sizes', () => {
  test('stepper matches one-row companion (600x80)', () => {
    expect(QUICK_ADD_WIDTH).toBe(600)
    expect(QUICK_ADD_STEPPER_HEIGHT).toBe(80)
  })

  test('save is taller than stepper', () => {
    expect(QUICK_ADD_SAVE_HEIGHT).toBe(120)
    expect(QUICK_ADD_SAVE_HEIGHT).toBeGreaterThan(QUICK_ADD_STEPPER_HEIGHT)
  })

  test('compact height for locked/success/no-books one-liners', () => {
    // QuickAddApp locked/success + QuickAddPage zero-entity empty state.
    expect(QUICK_ADD_COMPACT_HEIGHT).toBe(72)
    expect(QUICK_ADD_COMPACT_HEIGHT).toBeLessThan(QUICK_ADD_STEPPER_HEIGHT)
  })

  test('legacy spotlight companion heights are retired', () => {
    // 168 idle / 280 review belonged to the rejected stacked companion.
    expect(QUICK_ADD_STEPPER_HEIGHT).not.toBe(168)
    expect(QUICK_ADD_SAVE_HEIGHT).not.toBe(280)
  })
})
