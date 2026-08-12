import { describe, expect, test } from 'vitest'
import {
  QUICK_ADD_COMPACT_HEIGHT,
  QUICK_ADD_IDLE_HEIGHT,
  QUICK_ADD_REVIEW_HEIGHT,
  QUICK_ADD_WIDTH,
} from './quickAddWindow'

/**
 * Keep these in sync with `QUICK_ADD_*` in
 * `apps/desktop/src-tauri/src/tray.rs` (Rust idle default must match
 * QUICK_ADD_IDLE_HEIGHT / QUICK_ADD_WIDTH).
 */
describe('quickAddWindow sizes', () => {
  test('idle matches Spotlight companion (600x168)', () => {
    expect(QUICK_ADD_WIDTH).toBe(600)
    expect(QUICK_ADD_IDLE_HEIGHT).toBe(168)
  })

  test('review is taller than idle', () => {
    expect(QUICK_ADD_REVIEW_HEIGHT).toBe(280)
    expect(QUICK_ADD_REVIEW_HEIGHT).toBeGreaterThan(QUICK_ADD_IDLE_HEIGHT)
  })

  test('compact height for locked/success/no-books one-liners', () => {
    // QuickAddApp locked/success + QuickAddPage zero-entity empty state.
    expect(QUICK_ADD_COMPACT_HEIGHT).toBe(72)
    expect(QUICK_ADD_COMPACT_HEIGHT).toBeLessThan(QUICK_ADD_IDLE_HEIGHT)
  })
})
