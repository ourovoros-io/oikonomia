import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  BookOpen,
  FolderOpen,
  LayoutDashboard,
  Loader2,
  Lock,
  Plus,
  Receipt,
  Settings,
  Wallet,
} from 'lucide-react'
import { Aurora } from './components/Aurora'
import { Logo } from './components/Logo'
import { PageErrorBoundary } from './components/PageErrorBoundary'
import { UnlockScreen } from './components/UnlockScreen'
import { Button, ErrorBanner, ToastStack } from './components/ui'
import { bookDotColour } from './lib/bookColour'
import { cn } from './lib/cn'
import { TopBarContext, type TopBarSlots } from './lib/topBar'
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
import { DashboardPage } from './pages/DashboardPage'
import { TransactionsPage } from './pages/TransactionsPage'
import { DocumentsPage } from './pages/DocumentsPage'
import { AccountsPage } from './pages/AccountsPage'
import { ReportsPage } from './pages/ReportsPage'
import { SettingsPage } from './pages/SettingsPage'
import { DamagedPrefsNotice } from './components/DamagedPrefsNotice'
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

/** The book last open, or null when the preferences cannot be read. */
async function rememberedEntityId(): Promise<string | null> {
  try {
    return (await api.getUiPrefs()).last_entity_id
  } catch {
    return null
  }
}

export default function App() {
  const { t } = useI18n()
  const [active, setActive] = useState<NavId>('dashboard')
  const [status, setStatus] = useState<VaultStatus | null>(null)
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [locking, setLocking] = useState(false)
  /** The preferences file is damaged and the notice has not been dismissed. */
  const [prefsNotice, setPrefsNotice] = useState(false)
  const [entities, setEntities] = useState<Entity[]>([])
  const [entityId, setEntityId] = useState<string | null>(null)
  const [lockTimeoutSecs, setLockTimeoutSecs] = useState(15 * 60)
  const [createBookIntent, setCreateBookIntent] = useState(0)
  const [newEntryIntent, setNewEntryIntent] = useState(0)
  const [titleSlot, setTitleSlot] = useState<HTMLDivElement | null>(null)
  const [actionsSlot, setActionsSlot] = useState<HTMLDivElement | null>(null)
  const [titleClaims, setTitleClaims] = useState(0)

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

  // The sidebar's Quick add opens New entry on Transactions. As with the
  // create-book intent, the page resets it once handled, so a remount (main is
  // keyed by the active page) never replays it.
  const openNewEntry = useCallback(() => {
    setActive('transactions')
    setNewEntryIntent((n) => n + 1)
  }, [])

  const onNewEntryIntentHandled = useCallback(() => {
    setNewEntryIntent(0)
  }, [])

  const claimTitle = useCallback(() => {
    setTitleClaims((n) => n + 1)
    return () => setTitleClaims((n) => n - 1)
  }, [])

  const topBar = useMemo<TopBarSlots>(
    () => ({ title: titleSlot, actions: actionsSlot, claimTitle }),
    [titleSlot, actionsSlot, claimTitle],
  )

  // The book last open comes from the preferences file; it is only a
  // preference, so a missing or unreadable file falls back to the first book.
  const loadEntities = useCallback(async () => {
    const list = await api.entityList()
    const remembered = await rememberedEntityId()
    setEntities(list)
    setEntityId((prev) => {
      if (prev && list.some((e) => e.id === prev)) return prev
      if (remembered && list.some((e) => e.id === remembered)) return remembered
      return list[0]?.id ?? null
    })
  }, [])

  const selectEntity = useCallback((id: string) => {
    setEntityId(id)
    // Losing this write only costs the next unlock its book choice.
    void api.rememberLastEntity(id).catch(() => undefined)
  }, [])

  // Said once per unlock, wherever the user lands: the damage used to show
  // only on the Settings page, where nobody looks after a restore or a lock.
  const noteDamagedPrefs = useCallback(async () => {
    try {
      const prefs = await api.getUiPrefs()
      setPrefsNotice(prefs?.unreadable === true)
    } catch {
      /* optional: the notice is a courtesy */
    }
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
        await noteDamagedPrefs()
      }
    } catch (err) {
      setError(commandErrorMessage(err, 'app.failedBackend'))
    }
  }, [loadEntities, noteDamagedPrefs])

  useEffect(() => {
    void refresh()
  }, [refresh])

  // A reset in Settings repairs the file, so leaving a page asks again and the
  // notice does not outlive the damage.
  function navigate(id: NavId) {
    setActive(id)
    if (prefsNotice) void noteDamagedPrefs()
  }

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
      setError(commandErrorMessage(err, 'app.failedLock'))
    } finally {
      setLocking(false)
    }
  }

  const content =
    status === null ? (
      <div className="flex h-full items-center justify-center text-sm text-[var(--color-muted)]">
        {t('common.loading')}
      </div>
    ) : status === 'uninitialized' || status === 'locked' ? (
      <UnlockScreen
        status={status}
        supportEmail={info?.support_email}
        appVersion={info?.version}
        onUnlocked={(next) => {
          setStatus(next)
          void refresh()
        }}
      />
    ) : (
      <TopBarContext.Provider value={topBar}>
        <div className="flex h-full min-h-0 text-[var(--color-fg)]">
          <aside className="glass-pane my-3 ml-3 flex w-[var(--sidebar-w)] shrink-0 flex-col gap-5 overflow-y-auto rounded-[20px] px-3 py-4 [@media(max-height:700px)]:gap-3">
            <div className="flex items-center gap-3 px-3">
              <Logo className="size-8 shrink-0" />
              <div className="min-w-0 leading-tight">
                <div className="truncate text-base font-semibold tracking-tight">Oikonomia</div>
                <div className="truncate text-xs text-[var(--color-muted)]">{t('app.localLedger')}</div>
              </div>
            </div>

            <nav className="flex flex-col gap-0.5">
              {NAV.map((item) => {
                const isActive = active === item.id
                const Icon = item.icon

                return (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => navigate(item.id)}
                    aria-current={isActive ? 'page' : undefined}
                    className={cn(
                      'flex h-10 items-center gap-3 rounded-xl px-3 text-sm font-medium transition [@media(max-height:700px)]:h-8',
                      isActive
                        ? 'bg-[linear-gradient(90deg,rgba(46,230,166,0.2),rgba(55,213,255,0.08))] text-[var(--color-fg)] shadow-[inset_0_0_0_1px_rgba(46,230,166,0.35),0_0_24px_rgba(46,230,166,0.12)]'
                        : 'text-[var(--color-fg-secondary)] hover:bg-white/[0.05] hover:text-[var(--color-fg)]',
                    )}
                  >
                    <Icon
                      className={cn(
                        'size-[1.125rem] shrink-0',
                        isActive ? 'text-[var(--color-accent)]' : 'text-[var(--color-dim)]',
                      )}
                      strokeWidth={1.75}
                    />
                    <span className="truncate">{t(item.labelKey)}</span>
                  </button>
                )
              })}
            </nav>

            <section className="flex min-h-0 flex-1 flex-col gap-2" aria-labelledby="sidebar-books">
              <h2
                id="sidebar-books"
                className="px-3 font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase"
              >
                {t('app.book')}
              </h2>

              {entities.length === 0 ? (
                <p className="px-3 text-sm text-[var(--color-muted)]">{t('app.noEntitiesYet')}</p>
              ) : (
                <ul className="flex min-h-[6.5rem] flex-col gap-0.5 overflow-y-auto">
                  {entities.map((book) => {
                    const selected = book.id === entity?.id
                    const dot = bookDotColour(book.id)

                    return (
                      <li key={book.id}>
                        <button
                          type="button"
                          onClick={() => selectEntity(book.id)}
                          aria-current={selected ? 'true' : undefined}
                          title={`${book.name}, ${book.base_currency}`}
                          className={cn(
                            'flex h-10 w-full items-center gap-3 rounded-xl px-3 text-left text-sm transition [@media(max-height:700px)]:h-8',
                            selected
                              ? 'bg-white/[0.06] text-[var(--color-fg)]'
                              : 'text-[var(--color-fg-secondary)] hover:text-[var(--color-fg)]',
                          )}
                        >
                          {/* The dot sits in a nav-icon-sized slot so book names start
                              on the same column as the nav labels above. */}
                          <span aria-hidden className="flex size-[1.125rem] shrink-0 items-center justify-center">
                            <span
                              className="size-2 rounded-full"
                              style={{ background: dot, boxShadow: `0 0 10px ${dot}` }}
                            />
                          </span>
                          {/* The name and currency are two adjacent inline spans with no
                              intervening whitespace text node, so a screen reader's
                              accessible-name computation runs them together with no
                              separator ("HouseholdEUR"); a visually-hidden span holding
                              the whole "name, currency" string is the one reliable fix,
                              so the two visible spans are hidden from the same
                              computation and never counted twice. */}
                          <span aria-hidden className="min-w-0 flex-1 truncate">
                            {book.name}
                          </span>
                          <span aria-hidden className="font-mono text-[11px] text-[var(--color-muted)]">
                            {book.base_currency}
                          </span>
                          <span className="sr-only">{`${book.name}, ${book.base_currency}`}</span>
                        </button>
                      </li>
                    )
                  })}
                </ul>
              )}
            </section>

            <div className="space-y-1.5">
              <Button
                className="w-full"
                onClick={openNewEntry}
                disabled={!entity}
                aria-describedby={entity ? undefined : 'quick-add-hint'}
              >
                <Plus className="size-4" />
                {t('app.sidebar.quickAdd')}
              </Button>
              {entity ? null : (
                <p id="quick-add-hint" className="px-3 text-xs text-[var(--color-muted)]">
                  {t('app.sidebar.quickAddNeedsBook')}
                </p>
              )}
            </div>

            <div className="px-3 font-mono text-[10px] leading-tight tracking-[0.04em] text-[var(--color-muted)]">
              {info ? t('app.versionEncrypted', { version: info.version }) : 'Oikonomia'}
            </div>
          </aside>

          <div className="flex min-w-0 flex-1 flex-col">
            <header className="flex h-16 shrink-0 items-center justify-between gap-4 px-7">
              <div className="flex min-w-0 items-baseline gap-3">
                {/* A page rendering <TopBar> fills this slot and claims the title.
                    Hidden while empty, or its flex gap would shift the fallback title. */}
                <div ref={setTitleSlot} className="flex min-w-0 items-baseline gap-3 empty:hidden" />
                {titleClaims === 0 ? (
                  <>
                    <span className="truncate text-xl font-semibold tracking-tight">
                      {entity ? entity.name : t('app.noBookSelected')}
                    </span>
                    <span className="truncate text-sm text-[var(--color-fg-secondary)]">
                      {entity
                        ? t('app.entityChart', {
                            currency: entity.base_currency,
                            chart: t(`chart.${entity.chart_template}`),
                          })
                        : t('app.createEntityInSettings')}
                    </span>
                  </>
                ) : null}
              </div>
              <div className="flex shrink-0 items-center gap-3">
                <div ref={setActionsSlot} className="flex items-center gap-3 empty:hidden" />
                <button
                  type="button"
                  onClick={() => void onLock()}
                  disabled={locking}
                  aria-label={t('app.lockVault')}
                  // A visible word, not a tooltip: a native title outlives the button when
                  // locking replaces the screen under the pointer.
                  className="glass-pane inline-flex h-10 items-center justify-center gap-2 rounded-full px-4 text-sm font-medium text-[var(--color-fg)] transition hover:bg-white/[0.08] disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {locking ? (
                    <Loader2 className="size-4 animate-spin" />
                  ) : (
                    <Lock className="size-4" strokeWidth={1.75} />
                  )}
                  {locking ? t('app.locking') : t('app.lock')}
                </button>
              </div>
            </header>

            <main key={active} className="flex-1 overflow-auto [scrollbar-gutter:stable]">
              <div className="mx-auto max-w-6xl px-7 pt-2 pb-10">
                <ToastStack>
                  <ErrorBanner message={error} className="" onDismiss={() => setError(null)} />
                  {prefsNotice && active !== 'settings' ? (
                    <DamagedPrefsNotice
                      onOpenSettings={() => navigate('settings')}
                      onDismiss={() => setPrefsNotice(false)}
                    />
                  ) : null}
                </ToastStack>

                <PageErrorBoundary page={active} resetKey={`${active}:${entity?.id ?? 'none'}`}>
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
                      newEntryIntent={newEntryIntent}
                      onNewEntryIntentHandled={onNewEntryIntentHandled}
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
                      onEntitiesChange={async () => {
                        try {
                          await loadEntities()
                        } catch (err) {
                          setError(commandErrorMessage(err, 'app.failedBackend'))
                        }
                      }}
                      onSelectEntity={(id) => {
                        selectEntity(id)
                        setActive('dashboard')
                      }}
                    />
                  ) : null}
                </PageErrorBoundary>
              </div>
            </main>
          </div>
        </div>
      </TopBarContext.Provider>
    )

  return (
    <div className="relative h-full">
      {/* Mounted once, above every status branch, so a status change never
          remounts it. */}
      <Aurora />
      <div className="relative z-10 h-full">{content}</div>
    </div>
  )
}
