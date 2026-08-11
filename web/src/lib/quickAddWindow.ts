import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'
import { isTauri } from './tauri'

/** Single-row rolling tray (no horizontal scroll). */
export const QUICK_ADD_WIDTH = 420
export const QUICK_ADD_IDLE_HEIGHT = 48
export const QUICK_ADD_REVIEW_HEIGHT = 48

/** Resize the tray quick-add panel (no-op outside Tauri). */
export async function setQuickAddHeight(height: number): Promise<void> {
  if (!isTauri()) return
  try {
    await getCurrentWindow().setSize(new LogicalSize(QUICK_ADD_WIDTH, height))
  } catch {
    // Window may already be tearing down.
  }
}
