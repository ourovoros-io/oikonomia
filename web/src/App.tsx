import { useCallback, useEffect, useState } from 'react'
import {
  BookOpen,
  FolderOpen,
  LayoutDashboard,
  Lock,
  Receipt,
  Settings,
  Wallet,
} from 'lucide-react'
import { Decor } from './components/Decor'
import { Logo } from './components/Logo'
import { TrialBanner } from './components/TrialBanner'
import { UnlockScreen } from './components/UnlockScreen'
import { Button, Select } from './components/ui'
import { cn } from './lib/cn'
import { listen } from '@tauri-apps/api/event'
import {
  appInfo,
  isTauri,
  vaultLock,
  vaultStatus,
  vaultTouch,
  type AppInfo,
  type VaultStatus,
} from './lib/tauri'
import { api, type Entity } from './lib/api'
import { commandErrorMessage } from './lib/commandError'
import type { CommandError } from './lib/tauri'
import type { LicenseStatus } from './lib/license'
import { DashboardPage } from './pages/DashboardPage'
import { TransactionsPage } from './pages/TransactionsPage'
import { DocumentsPage } from './pages/DocumentsPage'
import { AccountsPage } from './pages/AccountsPage'
import { ReportsPage } from './pages/ReportsPage'
import { SettingsPage } from './pages/SettingsPage'
import { useI18n } from './lib/I18nProvider'

const NAV = [
  { id: 'dashboard', labelKey: 'nav.dashboard', icon: LayoutDashboard },
  { id: 'transactions', labelKey: 'nav.transactions', icon: Receipt },
  { id: 'documents', labelKey: 'nav.documents', icon: FolderOpen },
  { id: 'accounts', labelKey: 'nav.accounts', icon: Wallet },
  { id: 'reports', labelKey: 'nav.reports', icon: BookOpen },
  { id: 'settings', labelKey: 'nav.settings', icon: Settings },
] as const

type NavId = (typeof NAV)[number]['id']

export default function App() {
  const { t } = useI18n()
  const [active, setActive] = useState<NavId>('dashboard')
  const [status, setStatus] = useState<VaultStatus | null>(null)
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [locking, setLocking] = useState(false)
  const [entities, setEntities] = useState<Entity[]>([])
  const [entityId, setEntityId] = useState<string | null>(null)
  const [lockTimeoutSecs, setLockTimeoutSecs] = useState(15 * 60)
  const [createBookIntent, setCreateBookIntent] = useState(0)
  const [license, setLicense] = useState<LicenseStatus | null>(null)

  const entity = entities.find((e) => e.id === entityId) ?? entities[0] ?? null

  const openCreateBook = useCallback(() => {
    setActive('settings')
    setCreateBookIntent((n) => n + 1)
  }, [])

  // SettingsPage calls this once it has consumed the current intent (opened
  // the form, scrolled to it), so a later remount (main's key={active} tears
  // Settings down on every navigation) does not replay a stale intent and
  // reopen the dialog on every subsequent visit to Settings.
  const onCreateBookIntentHandled = useCallback(() => {
    setCreateBookIntent(0)
  }, [])

  const loadEntities = useCallback(async () => {
    const list = await api.entityList()
    setEntities(list)
    setEntityId((prev) => {
      if (prev && list.some((e) => e.id === prev)) return prev
      return list[0]?.id ?? null
    })
  }, [])

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextInfo] = await Promise.all([vaultStatus(), appInfo()])
      setStatus(nextStatus)
      setInfo(nextInfo)
      setError(null)
      if (nextStatus === 'unlocked') {
        await loadEntities()
        try {
          setLockTimeoutSecs(await api.getLockTimeout())
        } catch {
          /* optional */
        }
        try {
          setLicense(await api.licenseStatus())
        } catch {
          /* optional — TrialBanner simply stays hidden */
        }
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : t('app.failedBackend'))
    }
  }, [loadEntities])

  useEffect(() => {
    void refresh()
  }, [refresh])

  // The Rust watchdog is the authority on idle locking; it emits this event
  // when it closes the vault so the UI drops to the unlock screen.
  useEffect(() => {
    if (!isTauri()) return

    let unlisten: (() => void) | undefined
    let cancelled = false

    void listen('vault-locked', () => {
      setStatus('locked')
      setEntities([])
      setEntityId(null)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // Fast-path idle timer, plus a throttled heartbeat so the Rust watchdog
  // counts UI activity (mouse/keyboard) as activity, not just commands.
  useEffect(() => {
    if (status !== 'unlocked') return

    let timer: ReturnType<typeof setTimeout> | null = null
    let lastHeartbeat = 0

    const reset = () => {
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        void vaultLock().then((s) => setStatus(s))
      }, lockTimeoutSecs * 1000)

      const now = Date.now()
      if (now - lastHeartbeat > 60_000) {
        lastHeartbeat = now
        void vaultTouch().catch(() => undefined)
      }
    }

    const events = ['mousemove', 'keydown', 'click', 'scroll'] as const
    for (const ev of events) window.addEventListener(ev, reset)
    reset()

    return () => {
      if (timer) clearTimeout(timer)
      for (const ev of events) window.removeEventListener(ev, reset)
    }
  }, [status, lockTimeoutSecs])

  async function onLock() {
    setLocking(true)
    try {
      const next = await vaultLock()
      setStatus(next)
      setEntities([])
      setEntityId(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : t('app.failedLock'))
    } finally {
      setLocking(false)
    }
  }

  if (status === null) {
    return (
      <div className="flex h-full items-center justify-center bg-[var(--color-canvas)] text-sm text-[var(--color-muted)]">
        {t('common.loading')}
      </div>
    )
  }

  if (status === 'uninitialized' || status === 'locked') {
    return (
      <UnlockScreen
        status={status}
        supportEmail={info?.support_email}
        onUnlocked={(next) => {
          setStatus(next)
          void refresh()
        }}
      />
    )
  }

  return (
    <div className="relative flex h-full min-h-0 flex-col bg-[var(--color-canvas)] text-[var(--color-fg)]">
      <Decor />

      {/* Strip: brand cluster left, utilities right. Spans the full width, so
          the chassis reads as one plate rather than a sidebar beside a page. */}
      <header className="relative z-10 flex h-11 shrink-0 items-center justify-between gap-4 border-b border-[var(--color-border)] bg-[var(--color-canvas)]/85 px-4 backdrop-blur">
        <div className="flex min-w-0 items-center gap-2.5">
          <Logo className="size-5 shrink-0" />
          <span className="t-brand text-[var(--color-fg)]">Oikonomia</span>
          <span className="h-3 w-px shrink-0 bg-[var(--color-border-strong)]" aria-hidden="true" />
          <span className="t-caption truncate">{t('app.localLedger')}</span>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <div className="hidden w-56 sm:block">
            <Select
              value={entity?.id ?? ''}
              onChange={(e) => setEntityId(e.target.value || null)}
              aria-label={t('app.activeEntity')}
            >
              {entities.length === 0 ? <option value="">{t('app.noEntitiesYet')}</option> : null}
              {entities.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.name} · {e.base_currency}
                </option>
              ))}
            </Select>
          </div>
          <Button
            variant="secondary"
            onClick={() => void onLock()}
            disabled={locking}
            aria-label={t('app.lockVault')}
          >
            <Lock className="size-4" />
            {locking ? t('app.locking') : t('app.lock')}
          </Button>
        </div>
      </header>

      <div className="relative z-10 flex min-h-0 flex-1">
        {/* Index column: numbered rows, the whole row a hit target. The
            numbers are decoration for the eye, not for the screen reader —
            aria-hidden keeps each button's accessible name the bare label. */}
        <aside className="flex w-[var(--sidebar-w)] shrink-0 flex-col border-r border-[var(--color-border)] bg-[var(--color-surface)]/80">
          <nav className="flex flex-1 flex-col px-3 py-3">
            <div className="flex h-[22px] items-center justify-between border-b border-[var(--color-border-strong)]">
              <span className="t-caption-head">{t('app.book')}</span>
              <span className="t-tick">{String(NAV.length).padStart(2, '0')}</span>
            </div>

            {NAV.map((item, i) => {
              const isActive = active === item.id
              const Icon = item.icon
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => setActive(item.id)}
                  aria-current={isActive}
                  className={cn(
                    'grid min-h-[34px] w-full grid-cols-[22px_18px_minmax(0,1fr)_auto] items-center gap-x-2.5 border-b border-[var(--color-hair)] text-left transition-colors last:border-b-0',
                    isActive ? 'text-[var(--color-fg)]' : 'text-[var(--color-fg-secondary)] hover:text-[var(--color-fg)]',
                  )}
                >
                  <span className="t-tick" aria-hidden="true">
                    {String(i + 1).padStart(2, '0')}
                  </span>
                  <Icon
                    className={cn(
                      'size-4 shrink-0',
                      isActive ? 'text-[var(--color-accent)]' : 'text-[var(--color-dim)]',
                    )}
                    strokeWidth={1.75}
                  />
                  <span className={cn('t-name truncate', isActive && 'text-[var(--color-fg)]')}>
                    {t(item.labelKey)}
                  </span>
                  {isActive ? <span className="accent-bar" aria-hidden="true" /> : <span />}
                </button>
              )
            })}
          </nav>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <TrialBanner license={license} />
          <header className="flex h-12 shrink-0 items-center justify-between gap-4 border-b border-[var(--color-border)] px-6">
            <div className="flex min-w-0 items-baseline gap-3">
              <span className="t-value truncate text-[var(--color-fg)]">
                {entity ? entity.name : t('app.noBookSelected')}
              </span>
              <span className="t-caption truncate">
                {entity
                  ? t('app.entityChart', {
                      currency: entity.base_currency,
                      chart: t(`chart.${entity.chart_template}`),
                    })
                  : t('app.createEntityInSettings')}
              </span>
            </div>
          </header>

        <main key={active} className="flex-1 overflow-auto">
          <div className="mx-auto max-w-6xl px-6 py-8">
            {error ? (
              <div className="mb-5 rounded-xl border border-[var(--color-danger)]/30 bg-[var(--color-danger-soft)] px-4 py-3 text-sm text-[var(--color-danger)]">
                {error}
              </div>
            ) : null}

            {active === 'dashboard' ? (
              <DashboardPage
                key={entity?.id ?? 'none'}
                entity={entity}
                onCreateBook={openCreateBook}
              />
            ) : null}
            {active === 'transactions' ? (
              <TransactionsPage
                key={entity?.id ?? 'none'}
                entity={entity}
                onCreateBook={openCreateBook}
              />
            ) : null}
            {active === 'documents' ? (
              <DocumentsPage
                key={entity?.id ?? 'none'}
                entity={entity}
                onCreateBook={openCreateBook}
              />
            ) : null}
            {active === 'accounts' ? (
              <AccountsPage
                key={entity?.id ?? 'none'}
                entity={entity}
                onCreateBook={openCreateBook}
              />
            ) : null}
            {active === 'reports' ? (
              <ReportsPage
                key={entity?.id ?? 'none'}
                entity={entity}
                onCreateBook={openCreateBook}
              />
            ) : null}
            {active === 'settings' ? (
              <SettingsPage
                entities={entities}
                appInfo={info}
                createBookIntent={createBookIntent}
                onCreateBookIntentHandled={onCreateBookIntentHandled}
                onLockTimeoutChange={setLockTimeoutSecs}
                onLicenseChanged={setLicense}
                onEntitiesChange={async () => {
                  try {
                    await loadEntities()
                  } catch (err) {
                    setError(commandErrorMessage(err as CommandError))
                  }
                }}
                onSelectEntity={(id) => {
                  setEntityId(id)
                  setActive('dashboard')
                }}
              />
            ) : null}
          </div>
        </main>
        </div>
      </div>

      {/* Footer: livery mark left, build right, with the suite's 4 px hazard
          square. Series 01 / 01 — Oikonomia is its own one-product series. */}
      <footer className="relative z-10 flex h-11 shrink-0 items-center justify-between gap-4 border-t border-[var(--color-border)] bg-[var(--color-canvas)]/85 px-4 backdrop-blur">
        <span className="t-tick">SERIES 01 / 01</span>
        <div className="flex items-center gap-2.5">
          <span className="t-tick">
            {info ? t('app.versionEncrypted', { version: info.version }) : 'OIKONOMIA'}
          </span>
          <span className="size-1 bg-[var(--color-hazard)]" aria-hidden="true" />
        </div>
      </footer>
    </div>
  )
}
