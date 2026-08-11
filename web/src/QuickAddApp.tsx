import { useCallback, useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { api, formatMoney } from './lib/api'
import { isTauri, vaultStatus, type VaultStatus } from './lib/tauri'
import { Button } from './components/ui'
import { QuickAddPage, type QuickAddPosted } from './pages/QuickAddPage'

export default function QuickAddApp() {
  const [status, setStatus] = useState<VaultStatus | null>(null)
  const [dark, setDark] = useState(true)
  const [phase, setPhase] = useState<'form' | 'success'>('form')
  const [successLabel, setSuccessLabel] = useState('')
  const [formEpoch, setFormEpoch] = useState(0)
  const busyRef = useRef(false)
  const successTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  /** Debounced hide so native <select> / DateInput menus do not dismiss the panel. */
  const blurHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    void api
      .getTheme()
      .then((t) => setDark(t === 'dark'))
      .catch(() => undefined)
  }, [])

  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
  }, [dark])

  useEffect(() => {
    void vaultStatus()
      .then(setStatus)
      .catch(() => setStatus('locked'))
  }, [])

  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    void listen('vault-locked', () => {
      setStatus('locked')
      setPhase('form')
      if (successTimerRef.current) {
        clearTimeout(successTimerRef.current)
        successTimerRef.current = null
      }
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // Throttled activity heartbeat so the Rust idle watchdog sees tray input.
  useEffect(() => {
    if (status !== 'unlocked') return

    let lastHeartbeat = 0
    const touch = () => {
      const now = Date.now()
      if (now - lastHeartbeat > 60_000) {
        lastHeartbeat = now
        void vaultStatus().catch(() => undefined)
      }
    }

    const events = ['mousemove', 'keydown', 'click', 'scroll', 'pointerdown'] as const
    for (const ev of events) window.addEventListener(ev, touch)
    touch()

    return () => {
      for (const ev of events) window.removeEventListener(ev, touch)
    }
  }, [status])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      // Escape always dismisses when not mid-post (including success flash).
      if (busyRef.current) return
      if (blurHideTimerRef.current) {
        clearTimeout(blurHideTimerRef.current)
        blurHideTimerRef.current = null
      }
      if (successTimerRef.current) {
        clearTimeout(successTimerRef.current)
        successTimerRef.current = null
      }
      setPhase('form')
      void api.quickAddHide()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  // Native selects / calendar popovers often blur the webview on open.
  // Delay hide and cancel if focus returns, so choosing accounts stays open.
  useEffect(() => {
    const clearBlurHide = () => {
      if (blurHideTimerRef.current) {
        clearTimeout(blurHideTimerRef.current)
        blurHideTimerRef.current = null
      }
    }

    const onBlur = () => {
      if (busyRef.current) return
      clearBlurHide()
      blurHideTimerRef.current = setTimeout(() => {
        blurHideTimerRef.current = null
        if (busyRef.current) return
        // Focus returned (or never left the document) — keep the panel.
        if (document.hasFocus()) return
        void api.quickAddHide()
      }, 200)
    }

    const onFocus = () => {
      clearBlurHide()
    }

    window.addEventListener('blur', onBlur)
    window.addEventListener('focus', onFocus)
    return () => {
      window.removeEventListener('blur', onBlur)
      window.removeEventListener('focus', onFocus)
      clearBlurHide()
    }
  }, [])

  useEffect(() => {
    return () => {
      if (successTimerRef.current) clearTimeout(successTimerRef.current)
      if (blurHideTimerRef.current) clearTimeout(blurHideTimerRef.current)
    }
  }, [])

  const onBusyChange = useCallback((busy: boolean) => {
    busyRef.current = busy
  }, [])

  const onPosted = useCallback((info: QuickAddPosted) => {
    // Post finished: clear busy so Escape works during the success flash.
    busyRef.current = false
    const money = formatMoney(info.amountMinor, info.currency)
    setSuccessLabel(`Saved ${info.kind} ${money}`)
    setPhase('success')
    if (successTimerRef.current) clearTimeout(successTimerRef.current)
    successTimerRef.current = setTimeout(() => {
      successTimerRef.current = null
      void api.quickAddHide()
      setPhase('form')
      setFormEpoch((n) => n + 1)
    }, 1000)
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
        <p className="text-xs text-[var(--color-muted)]">
          Open Oikonomia to unlock, then try again.
        </p>
        <Button size="sm" onClick={() => void api.openMainWindow()}>
          Open Oikonomia
        </Button>
      </div>
    )
  }

  if (phase === 'success') {
    return (
      <div className="flex h-screen items-center justify-center bg-[var(--color-bg)] px-4 text-center">
        <p className="text-sm font-medium text-[var(--color-fg)]">{successLabel}</p>
      </div>
    )
  }

  return (
    <div className="h-screen overflow-hidden bg-[var(--color-bg)]">
      <QuickAddPage
        key={`${status}-${formEpoch}`}
        onPosted={onPosted}
        onBusyChange={onBusyChange}
      />
    </div>
  )
}
