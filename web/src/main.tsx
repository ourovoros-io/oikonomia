import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import './index.css'
import App from './App.tsx'
import QuickAddApp from './QuickAddApp.tsx'
import { I18nProvider } from './lib/I18nProvider.tsx'
import { isTauri } from './lib/tauri'

/** Set by the desktop shell (`tray.rs`) in a quick-add window that is not transparent. */
const QUICK_ADD_OPAQUE_FLAG = '__oikonomiaOpaqueQuickAdd'

const rootEl = document.getElementById('root')
if (!rootEl) {
  throw new Error('root element missing')
}
const root: HTMLElement = rootEl

async function mount() {
  let isQuickAdd = false
  if (isTauri()) {
    try {
      isQuickAdd = getCurrentWindow().label === 'quick-add'
    } catch {
      isQuickAdd = false
    }
  }

  // Transparent window + CSS shell draws rounded corners for the tray strip.
  // Where the shell could not ask for a transparent window (Linux, which may
  // have no compositor), it says so and the strip paints its own ground.
  if (isQuickAdd) {
    document.documentElement.classList.add('quick-add')
    document.body.classList.add('quick-add')
    if (Reflect.get(window, QUICK_ADD_OPAQUE_FLAG) === true) {
      document.documentElement.classList.add('quick-add-opaque')
    }
  }

  createRoot(root).render(
    <StrictMode>
      <I18nProvider>{isQuickAdd ? <QuickAddApp /> : <App />}</I18nProvider>
    </StrictMode>,
  )
}

void mount()
