import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { api } from './lib/api'
import { isTauri, vaultStatus, type VaultStatus } from './lib/tauri'
import { Button } from './components/ui'

export default function QuickAddApp() {
  const [status, setStatus] = useState<VaultStatus | null>(null)
  const [dark, setDark] = useState(true)

  useEffect(() => {
    void api.getTheme().then((t) => setDark(t === 'dark')).catch(() => undefined)
  }, [])

  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
  }, [dark])

  useEffect(() => {
    void vaultStatus().then(setStatus).catch(() => setStatus('locked'))
  }, [])

  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    void listen('vault-locked', () => setStatus('locked')).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') void api.quickAddHide()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  if (status === null) {
    return (
      <div className="flex h-screen items-center justify-center bg-[var(--color-bg)] text-xs text-[var(--color-muted)]">
        Loading…
      </div>
    )
  }

  if (status !== 'unlocked') {
    return (
      <div className="flex h-screen flex-col items-center justify-center gap-3 bg-[var(--color-bg)] px-4 text-center">
        <p className="text-sm text-[var(--color-fg)]">Vault is locked</p>
        <p className="text-xs text-[var(--color-muted)]">Open Oikonomia to unlock, then try again.</p>
        <Button size="sm" onClick={() => void api.openMainWindow()}>
          Open Oikonomia
        </Button>
      </div>
    )
  }

  return (
    <div className="flex h-screen items-center justify-center bg-[var(--color-bg)] text-xs text-[var(--color-muted)]">
      Quick add (form in next task)
    </div>
  )
}
