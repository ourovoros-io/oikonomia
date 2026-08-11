import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type FormEvent,
  type ReactNode,
} from 'react'
import { ChevronLeft } from 'lucide-react'
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
import { QUICK_ADD_IDLE_HEIGHT, setQuickAddHeight } from '../lib/quickAddWindow'
import { isTauri, type CommandError } from '../lib/tauri'
import { Button, Input } from '../components/ui'
import { DateInput } from '../components/DateInput'
import { cn } from '../lib/cn'
import {
  accountsOf,
  buildSimpleEntryInput,
  kindDefaultAccounts,
  lastAccountsMapKey,
  type AccountLike,
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

/** Decision order in the rolling flow. */
type Step = 'entity' | 'kind' | 'amount' | 'accounts' | 'memo' | 'review'

const KIND_OPTIONS: Array<{ id: EntryKind; label: string; hint: string }> = [
  { id: 'expense', label: 'Expense', hint: 'Paid now' },
  { id: 'income', label: 'Income', hint: 'Received' },
  { id: 'bill', label: 'Bill', hint: 'Payable' },
  { id: 'transfer', label: 'Transfer', hint: 'Move money' },
]

const compactControl =
  'h-8 w-full min-w-0 rounded-md border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] px-2 text-xs text-[var(--color-fg)] outline-none transition placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)] focus:ring-1 focus:ring-[var(--color-accent)]/25 disabled:opacity-50'

const MAX_DOC_BYTES = 8 * 1024 * 1024
const ROLL_MS = 260

function roleIdsFromLast(last: LastRoleAccounts): string[] {
  return [
    last.category_account_id,
    last.wallet_account_id,
    last.payable_account_id,
    last.from_account_id,
    last.to_account_id,
  ].filter((id): id is string => Boolean(id))
}

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

function stepLabel(step: Step): string {
  switch (step) {
    case 'entity':
      return 'Book'
    case 'kind':
      return 'Type'
    case 'amount':
      return 'Amount'
    case 'accounts':
      return 'Accounts'
    case 'memo':
      return 'Save'
    case 'review':
      return 'Document'
  }
}

export function QuickAddPage({ onPosted, onBusyChange }: Props) {
  const [entities, setEntities] = useState<Entity[]>([])
  const [entityId, setEntityId] = useState<string | null>(null)
  const [accounts, setAccounts] = useState<Account[]>([])
  const [prefs, setPrefs] = useState<UiPrefs | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  /** Active decision panel (after any roll settles). */
  const [step, setStep] = useState<Step>('entity')
  /** Panel currently sliding out (null when idle). */
  const [leaving, setLeaving] = useState<Step | null>(null)
  /** +1 = next (exit left / enter right), -1 = back (exit right / enter left). */
  const [rollDir, setRollDir] = useState<1 | -1>(1)
  const rollingRef = useRef(false)

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
  const [dragOver, setDragOver] = useState(false)
  const busyRef = useRef(false)
  const baseCurrencyRef = useRef('EUR')
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

  useEffect(() => {
    void setQuickAddHeight(QUICK_ADD_IDLE_HEIGHT)
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

  /** Roll to another decision: current exits one side, next enters from the other. */
  const rollTo = useCallback((next: Step, dir: 1 | -1) => {
    if (rollingRef.current) return
    setStep((cur) => {
      if (cur === next) return cur
      rollingRef.current = true
      setRollDir(dir)
      setLeaving(cur)
      window.setTimeout(() => {
        setLeaving(null)
        rollingRef.current = false
      }, ROLL_MS)
      return next
    })
    setError(null)
  }, [])

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
        // Always open on book selection so the first click starts the roll.
        setStep('entity')
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

  useEffect(() => {
    if (loading || step !== 'amount') return
    const t = window.setTimeout(() => {
      document.getElementById('quick-add-amount')?.focus()
    }, ROLL_MS + 20)
    return () => window.clearTimeout(t)
  }, [loading, step])

  async function selectEntity(nextId: string) {
    if (rollingRef.current) return
    setEntityId(nextId)
    setError(null)
    try {
      await loadAccountsFor(nextId, kind, prefs)
      rollTo('kind', 1)
    } catch (err) {
      setError((err as CommandError).message)
    }
  }

  function selectKind(next: EntryKind) {
    if (rollingRef.current) return
    setKind(next)
    if (next !== 'bill') setBillStatus('unpaid')
    if (!entity || !prefs) {
      applyRoleState(kindDefaultAccounts(next, accounts), roleSetters)
    } else {
      const key = lastAccountsMapKey(entity.id, next)
      const last = prefs.last_accounts_by_entity_kind[key]
      applyRoleState(resolveRoleAccounts(next, accounts, last), roleSetters)
    }
    rollTo('amount', 1)
  }

  function goBack() {
    if (rollingRef.current || formDisabled) return
    if (step === 'review') {
      clearDocumentReview()
      rollTo('entity', -1)
      return
    }
    if (step === 'kind') {
      rollTo('entity', -1)
      return
    }
    if (step === 'amount') {
      rollTo('kind', -1)
      return
    }
    if (step === 'accounts') {
      rollTo('amount', -1)
      return
    }
    if (step === 'memo') {
      rollTo('accounts', -1)
    }
  }

  function canGoBack(): boolean {
    return step !== 'entity' && !formDisabled
  }

  function applySuggestion(s: DocumentSuggestion, source: PendingDocSource) {
    setPendingDoc(source)
    setPendingAnalysis(JSON.stringify(s))
    setScanNotes(s.notes)

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

    const digits = currencyFractionDigits(baseCurrencyRef.current)
    if (s.amount_minor != null && s.amount_minor > 0 && digits === 2) {
      setAmount((s.amount_minor / 100).toFixed(2))
    }
    if (s.category_account_id) setCategoryId(s.category_account_id)
    if (s.wallet_account_id) setWalletId(s.wallet_account_id)
    if (s.payable_account_id) setPayableId(s.payable_account_id)
    // Caller already rolled to `review` (or we stay on that panel).
  }

  function clearDocumentReview() {
    setPendingDoc(null)
    setPendingAnalysis(null)
    setScanNotes(null)
    setAnalyzing(false)
    setDragOver(false)
  }

  function onCancelReview() {
    if (busy && !analyzing) return
    analyzeGenRef.current += 1
    busyRef.current = false
    setBusy(false)
    clearDocumentReview()
    setError(null)
    rollTo('entity', -1)
  }

  const processFile = useCallback(
    async (file: File, entId: string) => {
      if (busyRef.current || rollingRef.current) return
      if (file.size > MAX_DOC_BYTES) {
        setError('File too large (max 8 MB)')
        return
      }
      const gen = analyzeGenRef.current + 1
      analyzeGenRef.current = gen
      busyRef.current = true
      setBusy(true)
      setAnalyzing(true)
      setError(null)
      rollTo('review', 1)
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
        setError((err as CommandError).message || 'Could not analyze document')
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
    },
    [rollTo],
  )

  const processPath = useCallback(
    async (path: string, entId: string) => {
      if (busyRef.current || rollingRef.current) return
      const gen = analyzeGenRef.current + 1
      analyzeGenRef.current = gen
      busyRef.current = true
      setBusy(true)
      setAnalyzing(true)
      setError(null)
      rollTo('review', 1)
      try {
        const suggestion = await api.documentAnalyzePath({ entityId: entId, path })
        if (analyzeGenRef.current !== gen) return
        applySuggestion(suggestion, { kind: 'path', path })
      } catch (err) {
        if (analyzeGenRef.current !== gen) return
        setError((err as CommandError).message || 'Could not analyze document')
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
    },
    [rollTo],
  )

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
            if (path) void processPath(path, entId)
            else setError('No file path received from drop')
          }
        })
      } catch {
        // HTML5 fallback only.
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
    if (!isTauri()) setError('No file received')
  }

  async function onSubmit(ev?: FormEvent) {
    ev?.preventDefault()
    if (!entity || busy || analyzing) return

    const minor = parseMajorToMinor(amount, entity.base_currency)
    if (minor === null || minor <= 0) {
      setError('Enter a valid amount')
      if (step !== 'review') rollTo('amount', -1)
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

  function advanceFromAmount() {
    if (!entity) return
    const minor = parseMajorToMinor(amount, entity.base_currency)
    if (minor === null || minor <= 0) {
      setError('Enter a valid amount')
      return
    }
    rollTo('accounts', 1)
  }

  function advanceFromAccounts() {
    if (kind === 'expense' || kind === 'income') {
      if (!categoryId || !walletId) {
        setError('Pick both accounts')
        return
      }
    } else if (kind === 'bill') {
      if (!categoryId) {
        setError('Pick a category')
        return
      }
      if (billStatus === 'paid' && !walletId) {
        setError('Pick the wallet')
        return
      }
      if (billStatus === 'unpaid' && !payableId) {
        setError('Pick payable')
        return
      }
    } else if (kind === 'transfer') {
      if (!fromId || !toId) {
        setError('Pick both accounts')
        return
      }
      if (fromId === toId) {
        setError('From and To must differ')
        return
      }
    }
    rollTo('memo', 1)
  }

  const formDisabled = busy || analyzing

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
        <p className="text-xs text-[var(--color-fg)]">Create a book first</p>
        <Button size="sm" className="h-7" onClick={() => void api.openMainWindow()}>
          Open Oikonomia
        </Button>
      </div>
    )
  }

  const ccy = entity?.base_currency ?? 'EUR'

  function renderPanel(s: Step): ReactNode {
    switch (s) {
      case 'entity':
        return (
          <EntityPanel
            entities={entities}
            selectedId={entityId}
            disabled={formDisabled}
            onSelect={(id) => void selectEntity(id)}
          />
        )
      case 'kind':
        return <KindPanel disabled={formDisabled} onSelect={selectKind} />
      case 'amount':
        return (
          <AmountPanel
            ccy={ccy}
            amount={amount}
            date={date}
            disabled={formDisabled}
            onAmount={setAmount}
            onDate={setDate}
            onNext={advanceFromAmount}
          />
        )
      case 'accounts':
        return (
          <AccountsPanel
            kind={kind}
            billStatus={billStatus}
            formDisabled={formDisabled}
            categoryId={categoryId}
            walletId={walletId}
            payableId={payableId}
            fromId={fromId}
            toId={toId}
            expenseAccounts={expenseAccounts}
            incomeAccounts={incomeAccounts}
            walletAccounts={walletAccounts}
            assetWallets={assetWallets}
            payableAccounts={payableAccounts}
            transferAccounts={transferAccounts}
            setBillStatus={setBillStatus}
            setCategoryId={setCategoryId}
            setWalletId={setWalletId}
            setPayableId={setPayableId}
            setFromId={setFromId}
            setToId={setToId}
            onNext={advanceFromAccounts}
          />
        )
      case 'memo':
        return (
          <MemoPanel
            description={description}
            disabled={formDisabled}
            busy={busy}
            kind={kind}
            onDescription={setDescription}
            onSubmit={() => void onSubmit()}
          />
        )
      case 'review':
        return (
          <ReviewPanel
            analyzing={analyzing}
            pendingDoc={pendingDoc}
            scanNotes={scanNotes}
            formDisabled={formDisabled}
            busy={busy}
            ccy={ccy}
            amount={amount}
            date={date}
            description={description}
            kind={kind}
            billStatus={billStatus}
            categoryId={categoryId}
            walletId={walletId}
            payableId={payableId}
            expenseAccounts={expenseAccounts}
            incomeAccounts={incomeAccounts}
            walletAccounts={walletAccounts}
            assetWallets={assetWallets}
            payableAccounts={payableAccounts}
            setAmount={setAmount}
            setDate={setDate}
            setDescription={setDescription}
            setBillStatus={setBillStatus}
            setCategoryId={setCategoryId}
            setWalletId={setWalletId}
            setPayableId={setPayableId}
            onCancel={onCancelReview}
            onSubmit={() => void onSubmit()}
          />
        )
    }
  }

  return (
    <div
      onDragOver={(e) => {
        e.preventDefault()
        e.stopPropagation()
        if (!formDisabled && entityId) setDragOver(true)
      }}
      onDragEnter={(e) => {
        e.preventDefault()
        e.stopPropagation()
        if (!formDisabled && entityId) setDragOver(true)
      }}
      onDragLeave={(e) => {
        e.preventDefault()
        if (e.currentTarget === e.target) setDragOver(false)
      }}
      onDrop={onHtmlDrop}
      className={cn(
        'flex h-full flex-col overflow-hidden px-2.5 pb-2 pt-1.5 transition',
        dragOver && 'bg-[var(--color-accent-soft)]/40',
      )}
    >
      <header className="mb-1 flex shrink-0 items-center gap-1">
        {canGoBack() ? (
          <button
            type="button"
            onClick={goBack}
            className="inline-flex size-7 shrink-0 items-center justify-center rounded-md text-[var(--color-muted)] transition hover:bg-[var(--color-surface-2)] hover:text-[var(--color-fg)]"
            aria-label="Back"
          >
            <ChevronLeft className="size-4" />
          </button>
        ) : (
          <span className="size-7 shrink-0" aria-hidden />
        )}
        <p className="min-w-0 flex-1 truncate text-center text-[11px] font-semibold tracking-wide text-[var(--color-muted)]">
          {stepLabel(step)}
          {entity && step !== 'entity' ? (
            <span className="font-normal text-[var(--color-muted)]/70"> · {entity.name}</span>
          ) : null}
        </p>
        <span className="size-7 shrink-0" aria-hidden />
      </header>

      {/* Stage: leaving panel slides out, entering panel slides in. */}
      <div className="relative min-h-0 flex-1 overflow-hidden">
        {leaving ? (
          <div
            key={`leave-${leaving}`}
            className={cn(
              'absolute inset-0',
              rollDir === 1 ? 'qa-exit-left' : 'qa-exit-right',
            )}
            aria-hidden
          >
            {renderPanel(leaving)}
          </div>
        ) : null}
        <div
          key={`enter-${step}`}
          className={cn(
            'absolute inset-0',
            leaving
              ? rollDir === 1
                ? 'qa-enter-right'
                : 'qa-enter-left'
              : undefined,
          )}
        >
          {renderPanel(step)}
        </div>
      </div>

      {error ? (
        <p className="mt-1 shrink-0 truncate text-center text-[10px] text-[var(--color-danger)]" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  )
}

/* ─── Decision panels (one “card” each, full stage) ─── */

function EntityPanel({
  entities,
  selectedId,
  disabled,
  onSelect,
}: {
  entities: Entity[]
  selectedId: string | null
  disabled: boolean
  onSelect: (id: string) => void
}) {
  return (
    <div className="flex h-full flex-col justify-center gap-1.5">
      <p className="text-center text-[11px] text-[var(--color-muted)]">Choose a book</p>
      <div className="grid grid-cols-2 gap-1.5">
        {entities.slice(0, 4).map((e) => {
          const active = e.id === selectedId
          return (
            <button
              key={e.id}
              type="button"
              disabled={disabled}
              onClick={() => onSelect(e.id)}
              className={cn(
                'rounded-lg border px-2.5 py-2.5 text-left transition',
                active
                  ? 'border-[var(--color-accent)] bg-[var(--color-accent-soft)]'
                  : 'border-[var(--color-border-strong)] bg-[var(--color-surface-2)] hover:border-[var(--color-accent)]/50',
              )}
            >
              <span className="block truncate text-xs font-semibold text-[var(--color-fg)]">
                {e.name}
              </span>
              <span className="block text-[10px] text-[var(--color-muted)]">{e.base_currency}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

function KindPanel({
  disabled,
  onSelect,
}: {
  disabled: boolean
  onSelect: (k: EntryKind) => void
}) {
  return (
    <div className="grid h-full grid-cols-2 content-center gap-1.5">
      {KIND_OPTIONS.map((opt) => (
        <button
          key={opt.id}
          type="button"
          disabled={disabled}
          onClick={() => onSelect(opt.id)}
          className="rounded-lg border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] px-2.5 py-2.5 text-left transition hover:border-[var(--color-accent)]/50 hover:bg-[var(--color-accent-soft)]/30 disabled:opacity-50"
        >
          <span className="block text-xs font-semibold text-[var(--color-fg)]">{opt.label}</span>
          <span className="block text-[10px] text-[var(--color-muted)]">{opt.hint}</span>
        </button>
      ))}
    </div>
  )
}

function AmountPanel({
  ccy,
  amount,
  date,
  disabled,
  onAmount,
  onDate,
  onNext,
}: {
  ccy: string
  amount: string
  date: string
  disabled: boolean
  onAmount: (v: string) => void
  onDate: (v: string) => void
  onNext: () => void
}) {
  return (
    <form
      className="flex h-full flex-col justify-center gap-2"
      onSubmit={(e) => {
        e.preventDefault()
        onNext()
      }}
    >
      <div className="flex items-center gap-2">
        <Input
          id="quick-add-amount"
          inputMode="decimal"
          placeholder="0.00"
          value={amount}
          onChange={(e) => onAmount(e.target.value)}
          className="h-10 flex-1 px-3 text-base tabular-nums"
          aria-label={`Amount (${ccy})`}
          required
          disabled={disabled}
        />
        <span className="shrink-0 text-xs font-medium text-[var(--color-muted)]">{ccy}</span>
      </div>
      <div className="flex items-center gap-2">
        <div className="min-w-0 flex-1 [&_input]:h-8 [&_input]:text-xs [&_button]:h-7 [&_button]:w-7">
          <DateInput value={date} onChange={onDate} required disabled={disabled} />
        </div>
        <Button type="submit" size="sm" disabled={disabled} className="h-8 shrink-0 px-3">
          Next
        </Button>
      </div>
    </form>
  )
}

function AccountsPanel(props: {
  kind: EntryKind
  billStatus: BillStatusTray
  formDisabled: boolean
  categoryId: string
  walletId: string
  payableId: string
  fromId: string
  toId: string
  expenseAccounts: AccountLike[]
  incomeAccounts: AccountLike[]
  walletAccounts: AccountLike[]
  assetWallets: AccountLike[]
  payableAccounts: AccountLike[]
  transferAccounts: AccountLike[]
  setBillStatus: (v: BillStatusTray) => void
  setCategoryId: (v: string) => void
  setWalletId: (v: string) => void
  setPayableId: (v: string) => void
  setFromId: (v: string) => void
  setToId: (v: string) => void
  onNext: () => void
}) {
  const p = props
  return (
    <form
      className="flex h-full flex-col justify-center gap-1.5"
      onSubmit={(e) => {
        e.preventDefault()
        p.onNext()
      }}
    >
      {p.kind === 'bill' ? (
        <select
          className={compactControl}
          value={p.billStatus}
          onChange={(e) => p.setBillStatus(e.target.value as BillStatusTray)}
          disabled={p.formDisabled}
        >
          <option value="unpaid">Unpaid</option>
          <option value="paid">Paid</option>
        </select>
      ) : null}

      {p.kind === 'expense' || p.kind === 'bill' ? (
        <select
          className={compactControl}
          value={p.categoryId}
          onChange={(e) => p.setCategoryId(e.target.value)}
          disabled={p.formDisabled}
          required
          aria-label="Category"
        >
          {p.expenseAccounts.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      ) : null}

      {p.kind === 'income' ? (
        <select
          className={compactControl}
          value={p.categoryId}
          onChange={(e) => p.setCategoryId(e.target.value)}
          disabled={p.formDisabled}
          required
          aria-label="Income"
        >
          {p.incomeAccounts.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      ) : null}

      {p.kind === 'expense' || p.kind === 'income' || (p.kind === 'bill' && p.billStatus === 'paid') ? (
        <select
          className={compactControl}
          value={p.walletId}
          onChange={(e) => p.setWalletId(e.target.value)}
          disabled={p.formDisabled}
          required
          aria-label="Wallet"
        >
          {(p.kind === 'income' ? p.assetWallets : p.walletAccounts).map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      ) : null}

      {p.kind === 'bill' && p.billStatus === 'unpaid' ? (
        <select
          className={compactControl}
          value={p.payableId}
          onChange={(e) => p.setPayableId(e.target.value)}
          disabled={p.formDisabled}
          required
          aria-label="Payable"
        >
          {p.payableAccounts.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      ) : null}

      {p.kind === 'transfer' ? (
        <>
          <select
            className={compactControl}
            value={p.fromId}
            onChange={(e) => p.setFromId(e.target.value)}
            disabled={p.formDisabled}
            required
            aria-label="From"
          >
            {p.transferAccounts.map((a) => (
              <option key={a.id} value={a.id}>
                From · {a.name}
              </option>
            ))}
          </select>
          <select
            className={compactControl}
            value={p.toId}
            onChange={(e) => p.setToId(e.target.value)}
            disabled={p.formDisabled}
            required
            aria-label="To"
          >
            {p.transferAccounts.map((a) => (
              <option key={a.id} value={a.id}>
                To · {a.name}
              </option>
            ))}
          </select>
        </>
      ) : null}

      <Button type="submit" size="sm" disabled={p.formDisabled} className="h-8">
        Next
      </Button>
    </form>
  )
}

function MemoPanel({
  description,
  disabled,
  busy,
  kind,
  onDescription,
  onSubmit,
}: {
  description: string
  disabled: boolean
  busy: boolean
  kind: EntryKind
  onDescription: (v: string) => void
  onSubmit: () => void
}) {
  return (
    <form
      className="flex h-full flex-col justify-center gap-2"
      onSubmit={(e) => {
        e.preventDefault()
        onSubmit()
      }}
    >
      <Input
        value={description}
        onChange={(e) => onDescription(e.target.value)}
        placeholder="Memo (optional)"
        className="h-9 px-3 text-sm"
        disabled={disabled}
        autoFocus
      />
      <Button type="submit" size="sm" busy={busy} disabled={disabled} className="h-9">
        Save {kind}
      </Button>
    </form>
  )
}

function ReviewPanel(props: {
  analyzing: boolean
  pendingDoc: PendingDocSource | null
  scanNotes: string | null
  formDisabled: boolean
  busy: boolean
  ccy: string
  amount: string
  date: string
  description: string
  kind: EntryKind
  billStatus: BillStatusTray
  categoryId: string
  walletId: string
  payableId: string
  expenseAccounts: AccountLike[]
  incomeAccounts: AccountLike[]
  walletAccounts: AccountLike[]
  assetWallets: AccountLike[]
  payableAccounts: AccountLike[]
  setAmount: (v: string) => void
  setDate: (v: string) => void
  setDescription: (v: string) => void
  setBillStatus: (v: BillStatusTray) => void
  setCategoryId: (v: string) => void
  setWalletId: (v: string) => void
  setPayableId: (v: string) => void
  onCancel: () => void
  onSubmit: () => void
}) {
  const p = props
  if (p.analyzing) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2">
        <p className="text-xs text-[var(--color-muted)]">Analyzing…</p>
        <Button type="button" size="sm" variant="ghost" className="h-7 text-xs" onClick={p.onCancel}>
          Cancel
        </Button>
      </div>
    )
  }

  const cats = p.kind === 'income' ? p.incomeAccounts : p.expenseAccounts
  const wallets = p.kind === 'income' ? p.assetWallets : p.walletAccounts

  return (
    <form
      className="flex h-full flex-col justify-center gap-1"
      onSubmit={(e) => {
        e.preventDefault()
        p.onSubmit()
      }}
    >
      <div className="flex items-center justify-between gap-1">
        <p className="min-w-0 truncate text-[10px] font-medium text-[var(--color-fg)]">
          {p.pendingDoc ? pendingDocLabel(p.pendingDoc) : 'Could not analyze'}
        </p>
        <button
          type="button"
          onClick={p.onCancel}
          className="shrink-0 text-[10px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          Cancel
        </button>
      </div>
      <div className="flex gap-1">
        <Input
          inputMode="decimal"
          value={p.amount}
          onChange={(e) => p.setAmount(e.target.value)}
          className="h-7 w-[4.5rem] shrink-0 px-1.5 text-xs tabular-nums"
          disabled={p.formDisabled}
          required
        />
        <div className="min-w-0 flex-1 [&_input]:h-7 [&_input]:text-[11px] [&_button]:h-6 [&_button]:w-6">
          <DateInput value={p.date} onChange={p.setDate} required disabled={p.formDisabled} />
        </div>
      </div>
      <div className="flex gap-1">
        <select
          className={cn(compactControl, 'h-7 flex-1')}
          value={p.categoryId}
          onChange={(e) => p.setCategoryId(e.target.value)}
          disabled={p.formDisabled}
        >
          {cats.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
        {p.kind === 'bill' && p.billStatus === 'unpaid' ? (
          <select
            className={cn(compactControl, 'h-7 flex-1')}
            value={p.payableId}
            onChange={(e) => p.setPayableId(e.target.value)}
            disabled={p.formDisabled}
          >
            {p.payableAccounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        ) : (
          <select
            className={cn(compactControl, 'h-7 flex-1')}
            value={p.walletId}
            onChange={(e) => p.setWalletId(e.target.value)}
            disabled={p.formDisabled}
          >
            {wallets.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        )}
      </div>
      <Button
        type="submit"
        size="sm"
        busy={p.busy}
        disabled={p.formDisabled || !p.pendingDoc}
        className="h-7"
      >
        Save
      </Button>
    </form>
  )
}
