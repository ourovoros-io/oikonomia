import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import { isTauri } from './tauri'

/** Beautiful-UI inspired command strip — roomier idle, taller for review. */
export const QUICK_ADD_WIDTH = 460
export const QUICK_ADD_IDLE_HEIGHT = 148
export const QUICK_ADD_REVIEW_HEIGHT = 268

/** Resize the tray quick-add panel (no-op outside Tauri). */
export async function setQuickAddHeight(height: number): Promise<void> {
  if (!isTauri()) return
  try {
    await getCurrentWindow().setSize(new LogicalSize(QUICK_ADD_WIDTH, height))
  } catch {
    // Window may already be tearing down.
  }
}
