import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { listen } from '@tauri-apps/api/event'
import { api, formatMoney } from './lib/api'
import { isTauri, vaultStatus, type VaultStatus } from './lib/tauri'
import { Button } from './components/ui'
import { QUICK_ADD_IDLE_HEIGHT, setQuickAddHeight } from './lib/quickAddWindow'
import { QuickAddPage, type QuickAddPosted } from './pages/QuickAddPage'
import { cn } from './lib/cn'

/** Outer chrome for the transparent tray window (rounded, bordered, elevated). */
function Shell({
  children,
  className,
}: {
  children: ReactNode
  className?: string
}) {
  return (
    <div className="box-border flex h-full w-full items-stretch p-0">
      <div
        className={cn(
          'flex h-full w-full flex-col overflow-hidden',
          'rounded-[12px] border border-[var(--color-border-strong)]',
          'bg-[var(--color-surface)] text-[var(--color-fg)]',
          'shadow-[0_6px_20px_rgba(0,0,0,0.4)]',
          className,
        )}
      >
        {children}
      </div>
    </div>
  )
}

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
      busyRef.current = false
      if (successTimerRef.current) {
        clearTimeout(successTimerRef.current)
        successTimerRef.current = null
      }
      if (blurHideTimerRef.current) {
        clearTimeout(blurHideTimerRef.current)
        blurHideTimerRef.current = null
      }
      void setQuickAddHeight(QUICK_ADD_IDLE_HEIGHT)
      setStatus('locked')
      setPhase('form')
      setFormEpoch((n) => n + 1)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

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

  const hidePanel = useCallback(() => {
    if (successTimerRef.current) {
      clearTimeout(successTimerRef.current)
      successTimerRef.current = null
    }
    if (blurHideTimerRef.current) {
      clearTimeout(blurHideTimerRef.current)
      blurHideTimerRef.current = null
    }
    void setQuickAddHeight(QUICK_ADD_IDLE_HEIGHT)
    void api.quickAddHide()
    setPhase('form')
    setFormEpoch((n) => n + 1)
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      if (busyRef.current) return
      hidePanel()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [hidePanel])

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
        if (document.hasFocus()) return
        hidePanel()
      }, 200)
    }

    const onFocus = () => {
      clearBlurHide()
      void vaultStatus()
        .then(setStatus)
        .catch(() => setStatus('locked'))
    }

    window.addEventListener('blur', onBlur)
    window.addEventListener('focus', onFocus)
    return () => {
      window.removeEventListener('blur', onBlur)
      window.removeEventListener('focus', onFocus)
      clearBlurHide()
    }
  }, [hidePanel])

  useEffect(() => {
    return () => {
      if (successTimerRef.current) clearTimeout(successTimerRef.current)
      if (blurHideTimerRef.current) clearTimeout(blurHideTimerRef.current)
    }
  }, [])

  const onBusyChange = useCallback((busy: boolean) => {
    busyRef.current = busy
  }, [])

  const onPosted = useCallback(
    (info: QuickAddPosted) => {
      busyRef.current = false
      const money = formatMoney(info.amountMinor, info.currency)
      setSuccessLabel(`Saved ${info.kind} ${money}`)
      setPhase('success')
      if (successTimerRef.current) clearTimeout(successTimerRef.current)
      successTimerRef.current = setTimeout(() => {
        successTimerRef.current = null
        hidePanel()
      }, 1000)
    },
    [hidePanel],
  )

  if (status === null) {
    return (
      <Shell>
        <div className="flex h-full flex-col items-center justify-center gap-1 px-2 text-[11px] text-[var(--color-muted)]">
          Loading…
        </div>
      </Shell>
    )
  }

  if (status !== 'unlocked') {
    return (
      <Shell>
        <div className="flex h-full flex-col items-stretch justify-center gap-1.5 px-2.5 py-1.5">
          <p className="truncate text-center text-[11px] font-medium text-[var(--color-fg)]">
            Vault locked
          </p>
          <Button
            size="sm"
            className="h-8 w-full shrink-0 text-[11px]"
            onClick={() => void api.openMainWindow()}
          >
            Open Oikonomia
          </Button>
        </div>
      </Shell>
    )
  }

  if (phase === 'success') {
    return (
      <Shell>
        <div className="flex h-full flex-col items-center justify-center gap-0.5 px-2">
          <p className="truncate text-xs font-medium text-[var(--color-success)]">{successLabel}</p>
        </div>
      </Shell>
    )
  }

  return (
    <Shell>
      <QuickAddPage
        key={`${status}-${formEpoch}`}
        onPosted={onPosted}
        onBusyChange={onBusyChange}
      />
    </Shell>
  )
}
