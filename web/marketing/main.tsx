import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import '../src/index.css'
import './capture.css'
import App from '../src/App.tsx'
import { I18nProvider } from '../src/lib/I18nProvider.tsx'
import { LOCALE_STORAGE_KEY, parseLocale } from '../src/lib/i18n'
import { createHandler } from './fixture/handler'

const uiLocale = parseLocale(new URLSearchParams(window.location.search).get('lang'))
// fr and de have no demo ledger of their own, so they show the English one.
const demoLang = uiLocale === 'el' ? 'el' : 'en'

// Seed the i18n cache so the first paint is already in the requested language.
window.localStorage.setItem(LOCALE_STORAGE_KEY, uiLocale)

// createHandler's Args type is narrower than mockIPC's InvokeArgs (which also
// allows number[]/ArrayBuffer/Uint8Array for binary calls this app never makes),
// so the callback needs a param-type bridge to satisfy the contravariant check.
const answerCommand = createHandler(demoLang, uiLocale)

mockWindows('main')
mockIPC((cmd, payload) => answerCommand(cmd, payload as Record<string, unknown> | undefined), {
  shouldMockEvents: true,
})

const root = document.getElementById('root')
if (!root) throw new Error('root element missing')

createRoot(root).render(
  <StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </StrictMode>,
)
