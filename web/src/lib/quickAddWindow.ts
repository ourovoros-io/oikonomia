import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import { isTauri } from './tauri'

/**
 * One-row stepper companion window sizes (locked product constants).
 *
 * - Stepper 600×80 — vault / type / amount / accounts rolls
 * - Compact 600×72 — locked / success / no-books one-liners
 * - Save 600×120 — confirm row (bill status, memo, drop, Save/Cancel)
 * - Tray-anchored position only (native `tray.rs`; FE never recenters)
 * - Opaque soft card chrome (no backdrop-blur in v1)
 *
 * Native create size must stay in sync with `QUICK_ADD_*` in
 * `apps/desktop/src-tauri/src/tray.rs` (default before FE resize).
 */
export const QUICK_ADD_WIDTH = 600
export const QUICK_ADD_STEPPER_HEIGHT = 80
export const QUICK_ADD_COMPACT_HEIGHT = 72
export const QUICK_ADD_SAVE_HEIGHT = 120

/** Resize the tray quick-add panel (no-op outside Tauri). */
export async function setQuickAddHeight(height: number): Promise<void> {
  if (!isTauri()) return
  try {
    await getCurrentWindow().setSize(new LogicalSize(QUICK_ADD_WIDTH, height))
  } catch {
    // Window may already be tearing down.
  }
}
