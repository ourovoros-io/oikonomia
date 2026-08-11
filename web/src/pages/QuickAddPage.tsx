import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type FormEvent,
} from 'react'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import {
  api,
  todayISO,
  type Account,
  type DocumentSuggestion,
  type Entity,
  type LastRoleAccounts,
  type PendingDocSource,
  type UiPrefs,
} from '../lib/api'
import { currencyFractionDigits, parseMajorToMinor } from '../lib/money'
import { fileToBase64, mimeFromName } from '../lib/files'
import {
  QUICK_ADD_IDLE_HEIGHT,
  QUICK_ADD_REVIEW_HEIGHT,
  setQuickAddHeight,
} from '../lib/quickAddWindow'
import { isTauri, type CommandError } from '../lib/tauri'
import { Button, ErrorBanner, Input } from '../components/ui'
import { DateInput } from '../components/DateInput'
import { cn } from '../lib/cn'
import {
  accountsOf,
  buildSimpleEntryInput,
  kindDefaultAccounts,
  lastAccountsMapKey,
  type BillStatusTray,
  type EntryKind,
} from '../lib/simpleEntry'

export type QuickAddPosted = {
  kind: EntryKind
  amountMinor: number
  currency: string
}

type Props = {
  onPosted: (info: QuickAddPosted) => void
  onBusyChange?: (busy: boolean) => void
}

const KIND_OPTIONS: Array<{ id: EntryKind; label: string }> = [
  { id: 'expense', label: 'Expense' },
  { id: 'income', label: 'Income' },
  { id: 'bill', label: 'Bill' },
  { id: 'transfer', label: 'Transfer' },
]

const compactControl =
  'h-8 w-full min-w-0 rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] px-2 text-xs text-[var(--color-fg)] outline-none transition placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)] focus:ring-1 focus:ring-[var(--color-accent)]/25 disabled:opacity-50'

const MAX_DOC_BYTES = 8 * 1024 * 1024

function roleIdsFromLast(last: LastRoleAccounts): string[] {
  return [
    last.category_account_id,
    last.wallet_account_id,
    last.payable_account_id,
    last.from_account_id,
    last.to_account_id,
  ].filter((id): id is string => Boolean(id))
}

/** Prefer last-used role accounts when every non-empty id is still active. */
function resolveRoleAccounts(
  kind: EntryKind,
  list: Account[],
  last: LastRoleAccounts | undefined,
): ReturnType<typeof kindDefaultAccounts> {
  if (last) {
    const activeIds = new Set(list.filter((a) => a.is_active).map((a) => a.id))
    const ids = roleIdsFromLast(last)
    if (ids.length > 0 && ids.every((id) => activeIds.has(id))) {
      return {
        categoryId: last.category_account_id ?? '',
        walletId: last.wallet_account_id ?? '',
        payableId: last.payable_account_id ?? '',
        fromId: last.from_account_id ?? '',
        toId: last.to_account_id ?? '',
      }
    }
  }
  return kindDefaultAccounts(kind, list)
}

function applyRoleState(
  roles: ReturnType<typeof kindDefaultAccounts>,
  set: {
    setCategoryId: (v: string) => void
    setWalletId: (v: string) => void
    setPayableId: (v: string) => void
    setFromId: (v: string) => void
    setToId: (v: string) => void
  },
) {
  set.setCategoryId(roles.categoryId)
  set.setWalletId(roles.walletId)
  set.setPayableId(roles.payableId)
  set.setFromId(roles.fromId)
  set.setToId(roles.toId)
}

function pendingDocLabel(source: PendingDocSource): string {
  if (source.kind === 'file') return source.file.name
  const parts = source.path.split(/[/\\]/)
  return parts[parts.length - 1] || source.path
}

export function QuickAddPage({ onPosted, onBusyChange }: Props) {
  const [entities, setEntities] = useState<Entity[]>([])
  const [entityId, setEntityId] = useState<string | null>(null)
  const [accounts, setAccounts] = useState<Account[]>([])
  const [prefs, setPrefs] = useState<UiPrefs | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const [kind, setKind] = useState<EntryKind>('expense')
  const [billStatus, setBillStatus] = useState<BillStatusTray>('unpaid')
  const [date, setDate] = useState(todayISO())
  const [description, setDescription] = useState('')
  const [categoryId, setCategoryId] = useState('')
  const [walletId, setWalletId] = useState('')
  const [payableId, setPayableId] = useState('')
  const [fromId, setFromId] = useState('')
  const [toId, setToId] = useState('')
  const [amount, setAmount] = useState('')
  const [busy, setBusy] = useState(false)

  const [pendingDoc, setPendingDoc] = useState<PendingDocSource | null>(null)
  const [pendingAnalysis, setPendingAnalysis] = useState<string | null>(null)
  const [scanNotes, setScanNotes] = useState<string | null>(null)
  const [analyzing, setAnalyzing] = useState(false)
  /** True from drop until cancel/success — keeps height after analyzer errors. */
  const [reviewExpanded, setReviewExpanded] = useState(false)
  const [dragOver, setDragOver] = useState(false)
  const busyRef = useRef(false)
  const baseCurrencyRef = useRef('EUR')
  /** Bumped on cancel so in-flight analyze results are ignored. */
  const analyzeGenRef = useRef(0)

  const entity = useMemo(
    () => entities.find((e) => e.id === entityId) ?? null,
    [entities, entityId],
  )

  baseCurrencyRef.current = entity?.base_currency ?? 'EUR'

  const roleSetters = useMemo(
    () => ({ setCategoryId, setWalletId, setPayableId, setFromId, setToId }),
    [],
  )

  useEffect(() => {
    onBusyChange?.(busy || analyzing)
  }, [busy, analyzing, onBusyChange])

  // Keep panel height in sync with review mode (also re-apply on focus after hide).
  useEffect(() => {
    void setQuickAddHeight(reviewExpanded ? QUICK_ADD_REVIEW_HEIGHT : QUICK_ADD_IDLE_HEIGHT)
  }, [reviewExpanded])

  useEffect(() => {
    const onFocus = () => {
      void setQuickAddHeight(reviewExpanded ? QUICK_ADD_REVIEW_HEIGHT : QUICK_ADD_IDLE_HEIGHT)
    }
    window.addEventListener('focus', onFocus)
    return () => window.removeEventListener('focus', onFocus)
  }, [reviewExpanded])

  // Collapse to idle if this instance unmounts (success remount / lock).
  useEffect(() => {
    return () => {
      void setQuickAddHeight(QUICK_ADD_IDLE_HEIGHT)
    }
  }, [])

  const expenseAccounts = useMemo(() => accountsOf(accounts, ['expense']), [accounts])
  const incomeAccounts = useMemo(() => accountsOf(accounts, ['income']), [accounts])
  const walletAccounts = useMemo(
    () =>
      accountsOf(accounts, ['asset', 'liability']).filter((a) => {
        const n = a.name.toLowerCase()
        if (a.account_type === 'liability') {
          return (
            n.includes('card') || n.includes('payable') || n.includes('loan') || n.includes('bill')
          )
        }
        return true
      }),
    [accounts],
  )
  const assetWallets = useMemo(
    () => walletAccounts.filter((a) => a.account_type === 'asset'),
    [walletAccounts],
  )
  const payableAccounts = useMemo(() => {
    const liab = accountsOf(accounts, ['liability'])
    const preferred = liab.filter((a) => {
      const n = a.name.toLowerCase()
      return n.includes('payable') || n.includes('bill') || n.includes('ap')
    })
    return preferred.length > 0 ? preferred : liab
  }, [accounts])
  const transferAccounts = useMemo(() => accountsOf(accounts, ['asset']), [accounts])

  const loadAccountsFor = useCallback(
    async (entId: string, nextKind: EntryKind, uiPrefs: UiPrefs | null) => {
      const list = await api.accountList(entId)
      setAccounts(list)
      const key = lastAccountsMapKey(entId, nextKind)
      const last = uiPrefs?.last_accounts_by_entity_kind[key]
      applyRoleState(resolveRoleAccounts(nextKind, list, last), roleSetters)
      return list
    },
    [roleSetters],
  )

  // Initial load: entities + prefs, then accounts for resolved entity.
  useEffect(() => {
    let cancelled = false
    setLoading(true)
    setError(null)
    void (async () => {
      try {
        const [ents, uiPrefs] = await Promise.all([api.entityList(), api.getUiPrefs()])
        if (cancelled) return
        setEntities(ents)
        setPrefs(uiPrefs)
        if (ents.length === 0) {
          setEntityId(null)
          setAccounts([])
          return
        }
        const preferred =
          uiPrefs.last_entity_id && ents.some((e) => e.id === uiPrefs.last_entity_id)
            ? uiPrefs.last_entity_id
            : ents[0]!.id
        setEntityId(preferred)
        await loadAccountsFor(preferred, 'expense', uiPrefs)
      } catch (err) {
        if (!cancelled) setError((err as CommandError).message)
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [loadAccountsFor])

  // Focus amount once the form is ready (skip while document review is open).
  useEffect(() => {
    if (loading || !entityId || reviewExpanded) return
    const t = window.setTimeout(() => {
      document.getElementById('quick-add-amount')?.focus()
    }, 0)
    return () => window.clearTimeout(t)
  }, [loading, entityId, reviewExpanded])

  async function onEntityChange(nextId: string) {
    setEntityId(nextId)
    setError(null)
    try {
      await loadAccountsFor(nextId, kind, prefs)
    } catch (err) {
      setError((err as CommandError).message)
    }
  }

  function onKindChange(next: EntryKind) {
    setKind(next)
    if (next !== 'bill') setBillStatus('unpaid')
    if (!entity || !prefs) {
      applyRoleState(kindDefaultAccounts(next, accounts), roleSetters)
      return
    }
    const key = lastAccountsMapKey(entity.id, next)
    const last = prefs.last_accounts_by_entity_kind[key]
    applyRoleState(resolveRoleAccounts(next, accounts, last), roleSetters)
  }

  function applySuggestion(s: DocumentSuggestion, source: PendingDocSource) {
    setPendingDoc(source)
    setPendingAnalysis(JSON.stringify(s))
    setScanNotes(s.notes)
    setReviewExpanded(true)

    // Analyzer never suggests transfer; tray bill status is unpaid/paid only.
    if (s.kind === 'bill') {
      setKind('bill')
      setBillStatus(s.bill_unpaid ? 'unpaid' : 'paid')
    } else if (s.kind === 'income') {
      setKind('income')
      setBillStatus('unpaid')
    } else {
      setKind('expense')
      setBillStatus('unpaid')
    }

    if (s.entry_date) setDate(s.entry_date)
    if (s.description) setDescription(s.description)
    else if (s.merchant) setDescription(s.merchant)

    // Analyzer emits 2-exponent minor units; backend clears non-2-digit currencies.
    const digits = currencyFractionDigits(baseCurrencyRef.current)
    if (s.amount_minor != null && s.amount_minor > 0 && digits === 2) {
      setAmount((s.amount_minor / 100).toFixed(2))
    }
    if (s.category_account_id) setCategoryId(s.category_account_id)
    if (s.wallet_account_id) setWalletId(s.wallet_account_id)
    if (s.payable_account_id) setPayableId(s.payable_account_id)
  }

  function clearDocumentReview() {
    setPendingDoc(null)
    setPendingAnalysis(null)
    setScanNotes(null)
    setAnalyzing(false)
    setReviewExpanded(false)
    setDragOver(false)
  }

  function onCancelReview() {
    // Allow cancel after analyzer failure or mid-analyze; block only while a post is in flight.
    if (busy && !analyzing) return
    analyzeGenRef.current += 1
    busyRef.current = false
    setBusy(false)
    clearDocumentReview()
    setError(null)
    void setQuickAddHeight(QUICK_ADD_IDLE_HEIGHT)
  }

  const processFile = useCallback(async (file: File, entId: string) => {
    if (busyRef.current) return
    // Resource guard only — backend enforces the same cap (MAX_DOCUMENT_BYTES).
    if (file.size > MAX_DOC_BYTES) {
      setError('File too large (max 8 MB)')
      return
    }
    const gen = analyzeGenRef.current + 1
    analyzeGenRef.current = gen
    busyRef.current = true
    setBusy(true)
    setAnalyzing(true)
    setReviewExpanded(true)
    setError(null)
    void setQuickAddHeight(QUICK_ADD_REVIEW_HEIGHT)
    try {
      const dataBase64 = await fileToBase64(file)
      const mimeType = file.type || mimeFromName(file.name)
      const suggestion = await api.documentAnalyze({
        entityId: entId,
        filename: file.name,
        mimeType,
        dataBase64,
      })
      if (analyzeGenRef.current !== gen) return
      applySuggestion(suggestion, { kind: 'file', file })
    } catch (err) {
      if (analyzeGenRef.current !== gen) return
      const msg = (err as CommandError).message || 'Could not analyze document'
      setError(msg)
      // Stay expanded so the error is visible; Cancel collapses.
      setPendingDoc(null)
      setPendingAnalysis(null)
      setScanNotes(null)
    } finally {
      if (analyzeGenRef.current === gen) {
        busyRef.current = false
        setBusy(false)
        setAnalyzing(false)
      }
      setDragOver(false)
    }
  }, [])

  const processPath = useCallback(async (path: string, entId: string) => {
    if (busyRef.current) return
    const gen = analyzeGenRef.current + 1
    analyzeGenRef.current = gen
    busyRef.current = true
    setBusy(true)
    setAnalyzing(true)
    setReviewExpanded(true)
    setError(null)
    void setQuickAddHeight(QUICK_ADD_REVIEW_HEIGHT)
    try {
      const suggestion = await api.documentAnalyzePath({ entityId: entId, path })
      if (analyzeGenRef.current !== gen) return
      applySuggestion(suggestion, { kind: 'path', path })
    } catch (err) {
      if (analyzeGenRef.current !== gen) return
      const msg = (err as CommandError).message || 'Could not analyze document'
      setError(msg)
      setPendingDoc(null)
      setPendingAnalysis(null)
      setScanNotes(null)
    } finally {
      if (analyzeGenRef.current === gen) {
        busyRef.current = false
        setBusy(false)
        setAnalyzing(false)
      }
      setDragOver(false)
    }
  }, [])

  // Tauri webviews often give empty dataTransfer.files on OS file drops.
  useEffect(() => {
    if (!isTauri() || !entityId || loading) return

    let unlisten: (() => void) | undefined
    let cancelled = false
    const entId = entityId

    void (async () => {
      try {
        unlisten = await getCurrentWebview().onDragDropEvent((event) => {
          if (cancelled) return
          const payload = event.payload
          if (payload.type === 'enter' || payload.type === 'over') {
            setDragOver(true)
            return
          }
          if (payload.type === 'leave') {
            setDragOver(false)
            return
          }
          if (payload.type === 'drop') {
            setDragOver(false)
            const path = payload.paths[0]
            if (path) {
              void processPath(path, entId)
            } else {
              setError('No file path received from drop')
            }
          }
        })
      } catch {
        // HTML5 handlers remain as fallback.
      }
    })()

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [entityId, loading, processPath])

  function onHtmlDrop(e: DragEvent) {
    e.preventDefault()
    e.stopPropagation()
    setDragOver(false)
    if (!entityId || busyRef.current) return
    const file = e.dataTransfer.files?.[0]
    if (file && file.size > 0) {
      void processFile(file, entityId)
      return
    }
    if (!isTauri()) {
      setError('No file received')
    }
  }

  async function onSubmit(ev: FormEvent) {
    ev.preventDefault()
    if (!entity || busy || analyzing) return

    const minor = parseMajorToMinor(amount, entity.base_currency)
    if (minor === null || minor <= 0) {
      setError('Enter a valid amount (e.g. 25.50 or 25,50)')
      return
    }

    setBusy(true)
    busyRef.current = true
    setError(null)
    try {
      const input = buildSimpleEntryInput({
        entityId: entity.id,
        kind,
        billStatus,
        entryDate: date,
        description,
        amountMinor: minor,
        categoryId,
        walletId,
        payableId,
        fromId,
        toId,
      })

      if (pendingDoc?.kind === 'file') {
        const dataBase64 = await fileToBase64(pendingDoc.file)
        await api.entryPostSimpleWithDocument(
          input,
          {
            filename: pendingDoc.file.name,
            mimeType: pendingDoc.file.type || mimeFromName(pendingDoc.file.name),
            dataBase64,
          },
          pendingAnalysis ?? undefined,
        )
      } else if (pendingDoc?.kind === 'path') {
        await api.entryPostSimpleWithDocumentPath(
          input,
          pendingDoc.path,
          pendingAnalysis ?? undefined,
        )
      } else {
        await api.entryPostSimple(input)
      }

      const roles: LastRoleAccounts = {
        category_account_id: categoryId || null,
        wallet_account_id: walletId || null,
        payable_account_id: payableId || null,
        from_account_id: fromId || null,
        to_account_id: toId || null,
      }
      // Prefs are best-effort: a remember failure must not hide a successful post.
      try {
        await api.rememberQuickAdd(entity.id, kind, roles)
        setPrefs((prev) => {
          if (!prev) return prev
          return {
            ...prev,
            last_entity_id: entity.id,
            last_accounts_by_entity_kind: {
              ...prev.last_accounts_by_entity_kind,
              [lastAccountsMapKey(entity.id, kind)]: roles,
            },
          }
        })
      } catch {
        // ignore
      }

      clearDocumentReview()
      void setQuickAddHeight(QUICK_ADD_IDLE_HEIGHT)

      // Clear busy before success UI so parent Escape/blur are not stuck blocked.
      setBusy(false)
      busyRef.current = false
      onBusyChange?.(false)
      onPosted({
        kind,
        amountMinor: minor,
        currency: entity.base_currency,
      })
    } catch (err) {
      setError((err as CommandError).message)
      setBusy(false)
      busyRef.current = false
    }
  }

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--color-muted)]">
        Loading…
      </div>
    )
  }

  if (entities.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 px-3 text-center">
        <p className="text-xs text-[var(--color-fg)]">Create an entity in Oikonomia</p>
        <p className="text-[11px] text-[var(--color-muted)]">
          Quick add needs at least one book.
        </p>
        <Button size="sm" onClick={() => void api.openMainWindow()}>
          Open Oikonomia
        </Button>
      </div>
    )
  }

  if (!entity) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--color-muted)]">
        Loading…
      </div>
    )
  }

  const ccy = entity.base_currency
  const formDisabled = busy || analyzing

  return (
    <form
      onSubmit={(e) => void onSubmit(e)}
      onDragOver={(e) => {
        e.preventDefault()
        e.stopPropagation()
        if (!formDisabled) setDragOver(true)
      }}
      onDragEnter={(e) => {
        e.preventDefault()
        e.stopPropagation()
        if (!formDisabled) setDragOver(true)
      }}
      onDragLeave={(e) => {
        e.preventDefault()
        if (e.currentTarget === e.target) setDragOver(false)
      }}
      onDrop={onHtmlDrop}
      className={cn(
        'flex h-full flex-col gap-1 overflow-hidden p-1.5 transition',
        dragOver && 'ring-1 ring-inset ring-[var(--color-accent)] bg-[var(--color-accent-soft)]/40',
      )}
    >
      {/* Kind row */}
      <div className="inline-flex h-7 w-full items-stretch rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] p-0.5">
        {KIND_OPTIONS.map((opt) => {
          const active = kind === opt.id
          return (
            <button
              key={opt.id}
              type="button"
              aria-pressed={active}
              disabled={formDisabled}
              onClick={() => onKindChange(opt.id)}
              className={cn(
                'min-w-0 flex-1 rounded-[calc(var(--radius-control)-2px)] px-1 text-[11px] font-medium transition',
                active
                  ? 'bg-[var(--color-surface)] text-[var(--color-fg)] shadow-sm'
                  : 'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
              )}
            >
              {opt.label}
            </button>
          )
        })}
      </div>

      {/* Amount + role accounts + date + primary action */}
      <div className="flex min-w-0 items-center gap-1">
        <Input
          id="quick-add-amount"
          inputMode="decimal"
          placeholder={ccy}
          value={amount}
          onChange={(e) => setAmount(e.target.value)}
          className="h-8 w-[4.5rem] shrink-0 px-2 text-xs tabular-nums"
          aria-label={`Amount (${ccy})`}
          required
          disabled={formDisabled}
        />

        {kind === 'expense' ? (
          <>
            <select
              className={cn(compactControl, 'flex-1')}
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
              required
              disabled={formDisabled}
              aria-label="Category"
            >
              {expenseAccounts.length === 0 ? <option value="">No expense accounts</option> : null}
              {expenseAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            <select
              className={cn(compactControl, 'flex-1')}
              value={walletId}
              onChange={(e) => setWalletId(e.target.value)}
              required
              disabled={formDisabled}
              aria-label="Paid from"
            >
              {walletAccounts.length === 0 ? <option value="">No wallet accounts</option> : null}
              {walletAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </>
        ) : null}

        {kind === 'income' ? (
          <>
            <select
              className={cn(compactControl, 'flex-1')}
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
              required
              disabled={formDisabled}
              aria-label="Income type"
            >
              {incomeAccounts.length === 0 ? <option value="">No income accounts</option> : null}
              {incomeAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            <select
              className={cn(compactControl, 'flex-1')}
              value={walletId}
              onChange={(e) => setWalletId(e.target.value)}
              required
              disabled={formDisabled}
              aria-label="Received into"
            >
              {assetWallets.length === 0 ? <option value="">No asset accounts</option> : null}
              {assetWallets.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </>
        ) : null}

        {kind === 'bill' ? (
          <>
            <select
              className={cn(compactControl, 'w-[4.25rem] shrink-0')}
              value={billStatus}
              onChange={(e) => setBillStatus(e.target.value as BillStatusTray)}
              disabled={formDisabled}
              aria-label="Bill status"
            >
              <option value="unpaid">Unpaid</option>
              <option value="paid">Paid</option>
            </select>
            <select
              className={cn(compactControl, 'flex-1')}
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
              required
              disabled={formDisabled}
              aria-label="Bill category"
            >
              {expenseAccounts.length === 0 ? <option value="">No expense accounts</option> : null}
              {expenseAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            {billStatus === 'paid' ? (
              <select
                className={cn(compactControl, 'flex-1')}
                value={walletId}
                onChange={(e) => setWalletId(e.target.value)}
                required
                disabled={formDisabled}
                aria-label="Paid from"
              >
                {walletAccounts.length === 0 ? <option value="">No wallet accounts</option> : null}
                {walletAccounts.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
            ) : (
              <select
                className={cn(compactControl, 'flex-1')}
                value={payableId}
                onChange={(e) => setPayableId(e.target.value)}
                required
                disabled={formDisabled}
                aria-label="Bills payable"
              >
                {payableAccounts.length === 0 ? (
                  <option value="">No liability accounts</option>
                ) : null}
                {payableAccounts.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
            )}
          </>
        ) : null}

        {kind === 'transfer' ? (
          <>
            <select
              className={cn(compactControl, 'flex-1')}
              value={fromId}
              onChange={(e) => setFromId(e.target.value)}
              required
              disabled={formDisabled}
              aria-label="From"
            >
              {transferAccounts.length === 0 ? <option value="">No asset accounts</option> : null}
              {transferAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            <select
              className={cn(compactControl, 'flex-1')}
              value={toId}
              onChange={(e) => setToId(e.target.value)}
              required
              disabled={formDisabled}
              aria-label="To"
            >
              {transferAccounts.length === 0 ? <option value="">No asset accounts</option> : null}
              {transferAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </>
        ) : null}

        <div className="w-[5.75rem] shrink-0 [&_input]:h-8 [&_input]:px-2 [&_input]:pr-8 [&_input]:text-xs [&_button]:h-7 [&_button]:w-7">
          <DateInput value={date} onChange={setDate} required aria-label="Entry date" />
        </div>

        {pendingDoc ? (
          <Button type="submit" size="sm" busy={busy} className="h-8 shrink-0 px-2.5">
            Confirm & save
          </Button>
        ) : (
          <Button type="submit" size="sm" busy={busy || analyzing} className="h-8 shrink-0 px-2.5">
            {analyzing ? '…' : 'Add'}
          </Button>
        )}
      </div>

      {/* Memo + optional entity */}
      <div className="flex min-w-0 items-center gap-1">
        <Input
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Memo (optional)"
          className="h-8 flex-1 px-2 text-xs"
          aria-label="Memo"
          disabled={formDisabled}
        />
        {entities.length > 1 ? (
          <select
            className={cn(compactControl, 'w-[7.5rem] shrink-0')}
            value={entity.id}
            onChange={(e) => void onEntityChange(e.target.value)}
            disabled={formDisabled}
            aria-label="Book"
          >
            {entities.map((e) => (
              <option key={e.id} value={e.id}>
                {e.name}
              </option>
            ))}
          </select>
        ) : null}
      </div>

      {/* Idle drop affordance (subtle) or expanded document review */}
      {reviewExpanded ? (
        <div className="mt-0.5 flex min-h-0 flex-1 flex-col gap-1 overflow-auto rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface)] px-2 py-1.5">
          <div className="flex min-w-0 items-start justify-between gap-2">
            <div className="min-w-0">
              {analyzing ? (
                <p className="text-xs text-[var(--color-muted)]">Analyzing document…</p>
              ) : pendingDoc ? (
                <>
                  <p className="truncate text-xs font-medium text-[var(--color-fg)]">
                    {pendingDocLabel(pendingDoc)}
                  </p>
                  {scanNotes ? (
                    <p className="mt-0.5 line-clamp-3 text-[11px] leading-snug text-[var(--color-muted)]">
                      {scanNotes}
                    </p>
                  ) : (
                    <p className="mt-0.5 text-[11px] text-[var(--color-muted)]">
                      Document will be stored encrypted when you save. Review fields, then confirm.
                    </p>
                  )}
                </>
              ) : (
                <p className="text-xs text-[var(--color-muted)]">
                  Could not analyze. Cancel and try another file, or fill the form manually.
                </p>
              )}
            </div>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              disabled={busy && !analyzing}
              onClick={onCancelReview}
              className="h-7 shrink-0 px-2 text-xs"
            >
              Cancel
            </Button>
          </div>
        </div>
      ) : (
        <p className="px-0.5 text-center text-[10px] leading-none text-[var(--color-muted)]/80">
          Drop receipt to analyze
        </p>
      )}

      <ErrorBanner message={error} className="mb-0 px-2 py-1 text-xs" />
    </form>
  )
}
