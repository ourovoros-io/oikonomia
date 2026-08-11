import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import './index.css'
import App from './App.tsx'
import QuickAddApp from './QuickAddApp.tsx'
import { isTauri } from './lib/tauri'

const rootEl = document.getElementById('root')
if (!rootEl) {
  throw new Error('root element missing')
}
const root: HTMLElement = rootEl

document.documentElement.classList.add('dark')

async function mount() {
  let isQuickAdd = false
  if (isTauri()) {
    try {
      isQuickAdd = getCurrentWindow().label === 'quick-add'
    } catch {
      isQuickAdd = false
    }
  }

  createRoot(root).render(
    <StrictMode>{isQuickAdd ? <QuickAddApp /> : <App />}</StrictMode>,
  )
}

void mount()
