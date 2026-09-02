import { useCallback, useEffect, useState } from 'react'
import {
  BookOpen,
  FolderOpen,
  LayoutDashboard,
  Lock,
  Moon,
  Receipt,
  Settings,
  Sun,
  Wallet,
} from 'lucide-react'
import { Logo } from './components/Logo'
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
  const [dark, setDark] = useState(true)
  const [active, setActive] = useState<NavId>('dashboard')
  const [status, setStatus] = useState<VaultStatus | null>(null)
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [locking, setLocking] = useState(false)
  const [entities, setEntities] = useState<Entity[]>([])
  const [entityId, setEntityId] = useState<string | null>(null)
  const [lockTimeoutSecs, setLockTimeoutSecs] = useState(15 * 60)
  const [createBookIntent, setCreateBookIntent] = useState(0)

  const entity = entities.find((e) => e.id === entityId) ?? entities[0] ?? null

  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
  }, [dark])

  // Stored theme applies before unlock too; browser dev and first run keep
  // the dark default.
  useEffect(() => {
    void api
      .getTheme()
      .then((theme) => setDark(theme === 'dark'))
      .catch(() => undefined)
  }, [])

  function toggleTheme() {
    const next = !dark
    setDark(next)
    void api.setTheme(next ? 'dark' : 'light').catch(() => undefined)
  }

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
        onUnlocked={(next) => {
          setStatus(next)
          void refresh()
        }}
      />
    )
  }

  return (
    <div className="flex h-full min-h-0 bg-[var(--color-canvas)] text-[var(--color-fg)]">
      <aside className="flex w-[var(--sidebar-w)] shrink-0 flex-col border-r border-[var(--color-border)] bg-[var(--color-surface)]">
        <div className="flex h-14 items-center gap-2.5 border-b border-[var(--color-border)] px-4">
          <Logo className="size-8 shrink-0 rounded-lg shadow-sm shadow-[var(--color-accent)]/30" />
          <div className="min-w-0 leading-tight">
            <div className="truncate text-sm font-semibold tracking-tight">Oikonomia</div>
            <div className="truncate text-[11px] text-[var(--color-muted)]">{t('app.localLedger')}</div>
          </div>
        </div>

        <div className="border-b border-[var(--color-border)] px-3 py-3">
          <div className="mb-1.5 px-1 text-[11px] font-medium tracking-[0.12em] text-[var(--color-muted)] uppercase">
            {t('app.book')}
          </div>
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

        <nav className="flex flex-1 flex-col gap-0.5 p-2">
          {NAV.map((item) => {
            const isActive = active === item.id
            const Icon = item.icon
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => setActive(item.id)}
                className={cn(
                  'flex h-10 items-center gap-2.5 rounded-xl px-3 text-sm font-medium transition',
                  isActive
                    ? 'bg-[var(--color-accent-soft)] text-[var(--color-fg)] ring-1 ring-[var(--color-accent)]/20'
                    : 'text-[var(--color-fg-secondary)] hover:bg-[var(--color-surface-elevated)] hover:text-[var(--color-fg)]',
                )}
              >
                <Icon
                  className={cn(
                    'size-[1.125rem] shrink-0',
                    isActive ? 'text-[var(--color-accent)]' : 'text-[var(--color-muted)]',
                  )}
                  strokeWidth={1.75}
                />
                <span className="truncate">{t(item.labelKey)}</span>
              </button>
            )
          })}
        </nav>

        <div className="border-t border-[var(--color-border)] px-4 py-3 text-[11px] text-[var(--color-muted)]">
          {info ? t('app.versionEncrypted', { version: info.version }) : 'Oikonomia'}
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between gap-4 border-b border-[var(--color-border)] bg-[var(--color-surface)]/90 px-6 backdrop-blur">
          <div className="min-w-0">
            <div className="truncate text-sm font-medium text-[var(--color-fg)]">
              {entity ? entity.name : t('app.noBookSelected')}
            </div>
            <div className="truncate text-xs text-[var(--color-muted)]">
              {entity
                ? t('app.entityChart', {
                    currency: entity.base_currency,
                    chart: t(`chart.${entity.chart_template}`),
                  })
                : t('app.createEntityInSettings')}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <Button
              variant="secondary"
              size="icon"
              onClick={toggleTheme}
              aria-label={dark ? t('app.switchToLight') : t('app.switchToDark')}
              title={dark ? t('app.lightMode') : t('app.darkMode')}
            >
              {dark ? <Sun className="size-4" /> : <Moon className="size-4" />}
            </Button>
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
                createBookIntent={createBookIntent}
                onCreateBookIntentHandled={onCreateBookIntentHandled}
                onLockTimeoutChange={setLockTimeoutSecs}
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
  )
}
