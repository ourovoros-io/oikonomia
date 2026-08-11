import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import { isTauri } from './tauri'

export const QUICK_ADD_WIDTH = 420
/** Fixed height for the step wizard — one screen at a time, no scroll. */
export const QUICK_ADD_IDLE_HEIGHT = 168
/** Document review stays the same size (rolling fields, no tall expand). */
export const QUICK_ADD_REVIEW_HEIGHT = 168

/** Resize the tray quick-add panel (no-op outside Tauri). */
export async function setQuickAddHeight(height: number): Promise<void> {
  if (!isTauri()) return
  try {
    await getCurrentWindow().setSize(new LogicalSize(QUICK_ADD_WIDTH, height))
  } catch {
    // Window may already be tearing down.
  }
}
