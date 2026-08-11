import { describe, expect, test } from 'vitest'
import {
  QUICK_ADD_IDLE_HEIGHT,
  QUICK_ADD_REVIEW_HEIGHT,
  QUICK_ADD_WIDTH,
} from './quickAddWindow'

/**
 * Keep these in sync with `QUICK_ADD_*` in
 * `apps/desktop/src-tauri/src/tray.rs` (Rust idle default must match
 * QUICK_ADD_IDLE_HEIGHT).
 */
describe('quickAddWindow sizes', () => {
  test('idle matches Rust tray default (460x148)', () => {
    expect(QUICK_ADD_WIDTH).toBe(460)
    expect(QUICK_ADD_IDLE_HEIGHT).toBe(148)
  })

  test('review is taller than idle', () => {
    expect(QUICK_ADD_REVIEW_HEIGHT).toBe(268)
    expect(QUICK_ADD_REVIEW_HEIGHT).toBeGreaterThan(QUICK_ADD_IDLE_HEIGHT)
  })
})
