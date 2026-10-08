import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { listen } from '@tauri-apps/api/event'
import { api, formatMoney } from './lib/api'
import { isTauri, vaultStatus, vaultTouch, type VaultStatus } from './lib/tauri'
import { Button } from './components/ui'
import {
  QUICK_ADD_COMPACT_HEIGHT,
  QUICK_ADD_STEPPER_HEIGHT,
  setQuickAddHeight,
} from './lib/quickAddWindow'
import { QuickAddPage, type QuickAddPosted } from './pages/QuickAddPage'
import { cn } from './lib/cn'
import { trapTab } from './lib/focusTrap'
import { useI18n } from './lib/I18nProvider'

/**
 * Soft opaque BUI card: 16px radius, hairline border, no backdrop-blur.
 * OS window stays tray-anchored; outer transparent only for rounded corners.
 */
function Shell({
  children,
  className,
  appear = true,
}: {
  children: ReactNode
  className?: string
  appear?: boolean
}) {
  return (
    <div className="box-border flex h-full w-full items-stretch bg-transparent p-0">
      <div
        className={cn(
          'flex h-full w-full flex-col overflow-hidden',
          'rounded-[16px] border border-[var(--color-border-strong)]/65',
          'bg-[var(--color-surface)] text-[var(--color-fg)]',
          'shadow-[0_0_0_1px_rgba(255,255,255,0.04)_inset,0_14px_44px_rgba(0,0,0,0.55),0_2px_10px_rgba(0,0,0,0.3)]',
          appear && 'qa-appear',
          className,
        )}
      >
        {children}
      </div>
    </div>
  )
}

export default function QuickAddApp() {
  const { t } = useI18n()
  const [status, setStatus] = useState<VaultStatus | null>(null)
  const [phase, setPhase] = useState<'form' | 'success'>('form')
  const [successLabel, setSuccessLabel] = useState('')
  const [formEpoch, setFormEpoch] = useState(0)
  const busyRef = useRef(false)
  const successTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  /** Debounced hide so native <select> menus do not dismiss the panel. */
  const blurHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

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
      void setQuickAddHeight(QUICK_ADD_COMPACT_HEIGHT)
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
        void vaultTouch().catch(() => undefined)
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
    void setQuickAddHeight(QUICK_ADD_STEPPER_HEIGHT)
    void api.quickAddHide()
    setPhase('form')
    setFormEpoch((n) => n + 1)
  }, [])

  useEffect(() => {
    // Focus must not leave the strip: it hides on blur, and a Tab past the
    // last control would send the next keystrokes to the main window.
    const onTab = (e: KeyboardEvent) => trapTab(document, e)
    window.addEventListener('keydown', onTab)
    return () => window.removeEventListener('keydown', onTab)
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

  useEffect(() => {
    if (status === null) return
    if (status !== 'unlocked' || phase === 'success') {
      void setQuickAddHeight(QUICK_ADD_COMPACT_HEIGHT)
      return
    }
    void setQuickAddHeight(QUICK_ADD_STEPPER_HEIGHT)
  }, [status, phase])

  const onBusyChange = useCallback((busy: boolean) => {
    busyRef.current = busy
  }, [])

  const onPosted = useCallback(
    (info: QuickAddPosted) => {
      busyRef.current = false
      const money = formatMoney(info.amountMinor, info.currency)
      setSuccessLabel(t('quickAdd.saved', { kind: t(`kind.${info.kind}`), money }))
      setPhase('success')
      void setQuickAddHeight(QUICK_ADD_COMPACT_HEIGHT)
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
        <div className="flex h-full items-center justify-center px-2 text-[11px] text-[var(--color-muted)]">
          {t('common.loading')}
        </div>
      </Shell>
    )
  }

  if (status !== 'unlocked') {
    return (
      <Shell>
        <div className="flex h-full items-center gap-1.5 px-2">
          <p className="min-w-0 flex-1 text-[11px] leading-snug text-[var(--color-fg-secondary)]">
            {t('quickAdd.vaultLocked')}
          </p>
          <Button
            size="sm"
            className="h-7 shrink-0 rounded-full px-2.5 text-[11px]"
            onClick={() => void api.openMainWindow()}
          >
            {t('common.open')}
          </Button>
        </div>
      </Shell>
    )
  }

  if (phase === 'success') {
    return (
      <Shell appear={false}>
        <div className="qa-crossfade flex h-full items-center justify-center px-2">
          <p className="truncate text-[11px] font-medium text-[var(--color-success)]">
            {successLabel}
          </p>
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
        onDismiss={hidePanel}
      />
    </Shell>
  )
}
