import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import { isTauri } from './tauri'

/**
 * Spotlight companion window sizes (locked product constants).
 *
 * - Idle 600×168 — amount hero + account rows + memo always visible
 * - Review 600×280 — document analyze / confirm
 * - Compact 600×72 — locked / success one-liners
 * - Tray-anchored position only (native `tray.rs`; FE never recenters)
 * - Opaque soft card chrome (no backdrop-blur in v1)
 *
 * Idle default must stay in sync with `QUICK_ADD_*` in
 * `apps/desktop/src-tauri/src/tray.rs` (native create size before FE resize).
 */
export const QUICK_ADD_WIDTH = 600
export const QUICK_ADD_IDLE_HEIGHT = 168
export const QUICK_ADD_REVIEW_HEIGHT = 280
/** Locked / success one-liner chrome. */
export const QUICK_ADD_COMPACT_HEIGHT = 72

/** Resize the tray quick-add panel (no-op outside Tauri). */
export async function setQuickAddHeight(height: number): Promise<void> {
  if (!isTauri()) return
  try {
    await getCurrentWindow().setSize(new LogicalSize(QUICK_ADD_WIDTH, height))
  } catch {
    // Window may already be tearing down.
  }
}
