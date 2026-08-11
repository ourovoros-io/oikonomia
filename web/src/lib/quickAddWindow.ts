import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import { isTauri } from './tauri'

/** Compact one-shot command strip — two rows idle, taller while reviewing a drop. */
export const QUICK_ADD_WIDTH = 440
export const QUICK_ADD_IDLE_HEIGHT = 132
export const QUICK_ADD_REVIEW_HEIGHT = 248

/** Resize the tray quick-add panel (no-op outside Tauri). */
export async function setQuickAddHeight(height: number): Promise<void> {
  if (!isTauri()) return
  try {
    await getCurrentWindow().setSize(new LogicalSize(QUICK_ADD_WIDTH, height))
  } catch {
    // Window may already be tearing down.
  }
}
