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
import { ChevronLeft, FileUp } from 'lucide-react'
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

/** One screen at a time — rolling tray flow, no scroll. */
type Step = 'entity' | 'kind' | 'amount' | 'accounts' | 'memo' | 'review'

const KIND_OPTIONS: Array<{ id: EntryKind; label: string; hint: string }> = [
  { id: 'expense', label: 'Expense', hint: 'Paid now' },
  { id: 'income', label: 'Income', hint: 'Received' },
  { id: 'bill', label: 'Bill', hint: 'Payable' },
  { id: 'transfer', label: 'Transfer', hint: 'Between accounts' },
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

function stepTitle(step: Step, entityName: string | null): string {
  switch (step) {
    case 'entity':
      return 'Choose book'
    case 'kind':
      return entityName ? `Type · ${entityName}` : 'Type'
    case 'amount':
      return 'Amount'
    case 'accounts':
      return 'Accounts'
    case 'memo':
      return 'Memo'
    case 'review':
      return 'Review document'
  }
}

export function QuickAddPage({ onPosted, onBusyChange }: Props) {
  const [entities, setEntities] = useState<Entity[]>([])
  const [entityId, setEntityId] = useState<string | null>(null)
  const [accounts, setAccounts] = useState<Account[]>([])
  const [prefs, setPrefs] = useState<UiPrefs | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [step, setStep] = useState<Step>('kind')

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
  }, [])

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
        // Multi-book: start by confirming which book. Single book: jump to type.
        setStep(ents.length > 1 ? 'entity' : 'kind')
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
    }, 0)
    return () => window.clearTimeout(t)
  }, [loading, step])

  async function selectEntity(nextId: string) {
    setEntityId(nextId)
    setError(null)
    try {
      await loadAccountsFor(nextId, kind, prefs)
      setStep('kind')
    } catch (err) {
      setError((err as CommandError).message)
    }
  }

  function selectKind(next: EntryKind) {
    setKind(next)
    if (next !== 'bill') setBillStatus('unpaid')
    if (!entity || !prefs) {
      applyRoleState(kindDefaultAccounts(next, accounts), roleSetters)
    } else {
      const key = lastAccountsMapKey(entity.id, next)
      const last = prefs.last_accounts_by_entity_kind[key]
      applyRoleState(resolveRoleAccounts(next, accounts, last), roleSetters)
    }
    setStep('amount')
  }

  function goBack() {
    setError(null)
    if (step === 'review') {
      clearDocumentReview()
      setStep('kind')
      return
    }
    if (step === 'kind' && entities.length > 1) {
      setStep('entity')
      return
    }
    if (step === 'amount') {
      setStep('kind')
      return
    }
    if (step === 'accounts') {
      setStep('amount')
      return
    }
    if (step === 'memo') {
      setStep('accounts')
    }
  }

  function canGoBack(): boolean {
    if (step === 'entity') return false
    if (step === 'kind' && entities.length <= 1) return false
    return true
  }

  function applySuggestion(s: DocumentSuggestion, source: PendingDocSource) {
    setPendingDoc(source)
    setPendingAnalysis(JSON.stringify(s))
    setScanNotes(s.notes)
    setStep('review')

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
    setStep(entities.length > 1 ? 'entity' : 'kind')
  }

  const processFile = useCallback(async (file: File, entId: string) => {
    if (busyRef.current) return
    if (file.size > MAX_DOC_BYTES) {
      setError('File too large (max 8 MB)')
      return
    }
    const gen = analyzeGenRef.current + 1
    analyzeGenRef.current = gen
    busyRef.current = true
    setBusy(true)
    setAnalyzing(true)
    setStep('review')
    setError(null)
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
    setStep('review')
    setError(null)
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

  async function onSubmit(ev?: FormEvent) {
    ev?.preventDefault()
    if (!entity || busy || analyzing) return

    const minor = parseMajorToMinor(amount, entity.base_currency)
    if (minor === null || minor <= 0) {
      setError('Enter a valid amount (e.g. 25.50 or 25,50)')
      if (step !== 'review') setStep('amount')
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
    setError(null)
    if (!entity) return
    const minor = parseMajorToMinor(amount, entity.base_currency)
    if (minor === null || minor <= 0) {
      setError('Enter a valid amount (e.g. 25.50 or 25,50)')
      return
    }
    setStep('accounts')
  }

  function advanceFromAccounts() {
    setError(null)
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
        setError('Pick the wallet you paid from')
        return
      }
      if (billStatus === 'unpaid' && !payableId) {
        setError('Pick a payable account')
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
    setStep('memo')
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
        <p className="text-[11px] text-[var(--color-muted)]">Quick add needs at least one book.</p>
        <Button size="sm" onClick={() => void api.openMainWindow()}>
          Open Oikonomia
        </Button>
      </div>
    )
  }

  if (!entity && step !== 'entity') {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--color-muted)]">
        Loading…
      </div>
    )
  }

  const ccy = entity?.base_currency ?? 'EUR'
  const formDisabled = busy || analyzing
  const showChrome = step !== 'entity' || entities.length > 1

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
        'flex h-full flex-col overflow-hidden p-2 transition',
        dragOver && 'ring-1 ring-inset ring-[var(--color-accent)] bg-[var(--color-accent-soft)]/40',
      )}
    >
      {showChrome ? (
        <header className="mb-1.5 flex shrink-0 items-center gap-1">
          {canGoBack() ? (
            <button
              type="button"
              onClick={goBack}
              disabled={formDisabled}
              className="inline-flex size-7 shrink-0 items-center justify-center rounded-[var(--radius-control)] text-[var(--color-muted)] transition hover:bg-[var(--color-surface-2)] hover:text-[var(--color-fg)] disabled:opacity-50"
              aria-label="Back"
            >
              <ChevronLeft className="size-4" />
            </button>
          ) : (
            <span className="size-7 shrink-0" aria-hidden />
          )}
          <div className="min-w-0 flex-1">
            <p className="truncate text-xs font-semibold text-[var(--color-fg)]">
              {stepTitle(step, entity?.name ?? null)}
            </p>
          </div>
          <StepDots step={step} multiEntity={entities.length > 1} />
        </header>
      ) : null}

      <div className="min-h-0 flex-1 overflow-hidden">
        {step === 'entity' ? (
          <EntityStep
            entities={entities}
            selectedId={entityId}
            disabled={formDisabled}
            onSelect={(id) => void selectEntity(id)}
          />
        ) : null}

        {step === 'kind' ? (
          <KindStep disabled={formDisabled} onSelect={selectKind} />
        ) : null}

        {step === 'amount' ? (
          <AmountStep
            ccy={ccy}
            amount={amount}
            date={date}
            disabled={formDisabled}
            onAmount={setAmount}
            onDate={setDate}
            onNext={advanceFromAmount}
          />
        ) : null}

        {step === 'accounts' ? (
          <AccountsStep
            kind={kind}
            billStatus={billStatus}
            formDisabled={formDisabled}
            compactControl={compactControl}
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
        ) : null}

        {step === 'memo' ? (
          <MemoStep
            description={description}
            disabled={formDisabled}
            busy={busy}
            kind={kind}
            onDescription={setDescription}
            onSubmit={() => void onSubmit()}
          />
        ) : null}

        {step === 'review' ? (
          <ReviewStep
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
            compactControl={compactControl}
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
        ) : null}
      </div>

      {error ? <ErrorBanner message={error} className="mt-1 mb-0 shrink-0 px-2 py-1 text-xs" /> : null}

      {step === 'entity' || step === 'kind' ? (
        <p className="mt-1 flex shrink-0 items-center justify-center gap-1 text-[10px] text-[var(--color-muted)]/80">
          <FileUp className="size-3 opacity-70" aria-hidden />
          Drop a receipt anytime after you pick a book
        </p>
      ) : null}
    </div>
  )
}

function StepDots({ step, multiEntity }: { step: Step; multiEntity: boolean }) {
  const manual: Step[] = multiEntity
    ? ['entity', 'kind', 'amount', 'accounts', 'memo']
    : ['kind', 'amount', 'accounts', 'memo']
  if (step === 'review') {
    return (
      <span className="shrink-0 text-[10px] font-medium text-[var(--color-accent)]">Doc</span>
    )
  }
  const idx = manual.indexOf(step)
  return (
    <div className="flex shrink-0 items-center gap-1" aria-hidden>
      {manual.map((s, i) => (
        <span
          key={s}
          className={cn(
            'size-1.5 rounded-full transition',
            i === idx
              ? 'bg-[var(--color-accent)]'
              : i < idx
                ? 'bg-[var(--color-accent)]/50'
                : 'bg-[var(--color-border-strong)]',
          )}
        />
      ))}
    </div>
  )
}

function EntityStep({
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
    <div className="flex h-full flex-col gap-1">
      <p className="text-[11px] text-[var(--color-muted)]">Which book is this for?</p>
      <div className="grid min-h-0 flex-1 grid-cols-2 content-start gap-1.5 overflow-hidden">
        {entities.slice(0, 4).map((e) => {
          const active = e.id === selectedId
          return (
            <button
              key={e.id}
              type="button"
              disabled={disabled}
              onClick={() => onSelect(e.id)}
              className={cn(
                'rounded-[var(--radius-control)] border px-2 py-2 text-left transition',
                active
                  ? 'border-[var(--color-accent)] bg-[var(--color-accent-soft)]'
                  : 'border-[var(--color-border-strong)] bg-[var(--color-surface-2)] hover:border-[var(--color-accent)]/60',
              )}
            >
              <span className="block truncate text-xs font-medium text-[var(--color-fg)]">
                {e.name}
              </span>
              <span className="block text-[10px] text-[var(--color-muted)]">{e.base_currency}</span>
            </button>
          )
        })}
      </div>
      {entities.length > 4 ? (
        <p className="text-[10px] text-[var(--color-muted)]">
          Showing first 4 books. Open the full app for more.
        </p>
      ) : null}
    </div>
  )
}

function KindStep({
  disabled,
  onSelect,
}: {
  disabled: boolean
  onSelect: (k: EntryKind) => void
}) {
  return (
    <div className="grid h-full grid-cols-2 content-stretch gap-1.5">
      {KIND_OPTIONS.map((opt) => (
        <button
          key={opt.id}
          type="button"
          disabled={disabled}
          onClick={() => onSelect(opt.id)}
          className="flex flex-col justify-center rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] px-2.5 py-2 text-left transition hover:border-[var(--color-accent)]/60 hover:bg-[var(--color-accent-soft)]/40 disabled:opacity-50"
        >
          <span className="text-xs font-semibold text-[var(--color-fg)]">{opt.label}</span>
          <span className="text-[10px] text-[var(--color-muted)]">{opt.hint}</span>
        </button>
      ))}
    </div>
  )
}

function AmountStep({
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
          autoFocus
        />
        <span className="shrink-0 text-xs font-medium text-[var(--color-muted)]">{ccy}</span>
      </div>
      <div className="flex items-center gap-2">
        <div className="min-w-0 flex-1 [&_input]:h-8 [&_input]:px-2 [&_input]:pr-8 [&_input]:text-xs [&_button]:h-7 [&_button]:w-7">
          <DateInput
            value={date}
            onChange={onDate}
            required
            disabled={disabled}
            aria-label="Entry date"
          />
        </div>
        <Button type="submit" size="sm" disabled={disabled} className="h-8 shrink-0 px-3">
          Next
        </Button>
      </div>
    </form>
  )
}

function AccountsStep(props: {
  kind: EntryKind
  billStatus: BillStatusTray
  formDisabled: boolean
  compactControl: string
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
  const {
    kind,
    billStatus,
    formDisabled,
    compactControl,
    categoryId,
    walletId,
    payableId,
    fromId,
    toId,
    expenseAccounts,
    incomeAccounts,
    walletAccounts,
    assetWallets,
    payableAccounts,
    transferAccounts,
    setBillStatus,
    setCategoryId,
    setWalletId,
    setPayableId,
    setFromId,
    setToId,
    onNext,
  } = props

  let fields: ReactNode = null

  if (kind === 'expense') {
    fields = (
      <>
        <FieldSelect
          label="Category"
          className={compactControl}
          value={categoryId}
          onChange={setCategoryId}
          disabled={formDisabled}
          options={expenseAccounts}
          empty="No expense accounts"
        />
        <FieldSelect
          label="Paid from"
          className={compactControl}
          value={walletId}
          onChange={setWalletId}
          disabled={formDisabled}
          options={walletAccounts}
          empty="No wallet accounts"
        />
      </>
    )
  } else if (kind === 'income') {
    fields = (
      <>
        <FieldSelect
          label="Income type"
          className={compactControl}
          value={categoryId}
          onChange={setCategoryId}
          disabled={formDisabled}
          options={incomeAccounts}
          empty="No income accounts"
        />
        <FieldSelect
          label="Received into"
          className={compactControl}
          value={walletId}
          onChange={setWalletId}
          disabled={formDisabled}
          options={assetWallets}
          empty="No asset accounts"
        />
      </>
    )
  } else if (kind === 'bill') {
    fields = (
      <>
        <div className="flex items-center gap-2">
          <label className="shrink-0 text-[11px] text-[var(--color-muted)]">Status</label>
          <select
            className={cn(compactControl, 'flex-1')}
            value={billStatus}
            onChange={(e) => setBillStatus(e.target.value as BillStatusTray)}
            disabled={formDisabled}
            aria-label="Bill status"
          >
            <option value="unpaid">Unpaid</option>
            <option value="paid">Paid</option>
          </select>
        </div>
        <FieldSelect
          label="Category"
          className={compactControl}
          value={categoryId}
          onChange={setCategoryId}
          disabled={formDisabled}
          options={expenseAccounts}
          empty="No expense accounts"
        />
        {billStatus === 'paid' ? (
          <FieldSelect
            label="Paid from"
            className={compactControl}
            value={walletId}
            onChange={setWalletId}
            disabled={formDisabled}
            options={walletAccounts}
            empty="No wallet accounts"
          />
        ) : (
          <FieldSelect
            label="Payable"
            className={compactControl}
            value={payableId}
            onChange={setPayableId}
            disabled={formDisabled}
            options={payableAccounts}
            empty="No liability accounts"
          />
        )}
      </>
    )
  } else {
    fields = (
      <>
        <FieldSelect
          label="From"
          className={compactControl}
          value={fromId}
          onChange={setFromId}
          disabled={formDisabled}
          options={transferAccounts}
          empty="No asset accounts"
        />
        <FieldSelect
          label="To"
          className={compactControl}
          value={toId}
          onChange={setToId}
          disabled={formDisabled}
          options={transferAccounts}
          empty="No asset accounts"
        />
      </>
    )
  }

  return (
    <form
      className="flex h-full flex-col gap-1.5"
      onSubmit={(e) => {
        e.preventDefault()
        onNext()
      }}
    >
      <div className="flex min-h-0 flex-1 flex-col justify-center gap-1.5 overflow-hidden">
        {fields}
      </div>
      <Button type="submit" size="sm" disabled={formDisabled} className="h-8 shrink-0">
        Next
      </Button>
    </form>
  )
}

function FieldSelect({
  label,
  className,
  value,
  onChange,
  disabled,
  options,
  empty,
}: {
  label: string
  className: string
  value: string
  onChange: (v: string) => void
  disabled: boolean
  options: AccountLike[]
  empty: string
}) {
  return (
    <div className="flex min-w-0 items-center gap-2">
      <label className="w-16 shrink-0 text-[11px] text-[var(--color-muted)]">{label}</label>
      <select
        className={cn(className, 'flex-1')}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        required
        disabled={disabled}
        aria-label={label}
      >
        {options.length === 0 ? <option value="">{empty}</option> : null}
        {options.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}
          </option>
        ))}
      </select>
    </div>
  )
}

function MemoStep({
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
        aria-label="Memo"
        disabled={disabled}
        autoFocus
      />
      <Button type="submit" size="sm" busy={busy} disabled={disabled} className="h-9">
        Save {kind}
      </Button>
    </form>
  )
}

function ReviewStep(props: {
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
  compactControl: string
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
  const {
    analyzing,
    pendingDoc,
    scanNotes,
    formDisabled,
    busy,
    ccy,
    amount,
    date,
    description,
    kind,
    billStatus,
    categoryId,
    walletId,
    payableId,
    expenseAccounts,
    incomeAccounts,
    walletAccounts,
    assetWallets,
    payableAccounts,
    compactControl,
    setAmount,
    setDate,
    setDescription,
    setBillStatus,
    setCategoryId,
    setWalletId,
    setPayableId,
    onCancel,
    onSubmit,
  } = props

  if (analyzing) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
        <p className="text-xs text-[var(--color-muted)]">Analyzing document…</p>
        <Button type="button" size="sm" variant="ghost" onClick={onCancel} className="h-7 text-xs">
          Cancel
        </Button>
      </div>
    )
  }

  const categoryOpts =
    kind === 'income' ? incomeAccounts : expenseAccounts
  const walletOpts = kind === 'income' ? assetWallets : walletAccounts

  return (
    <form
      className="flex h-full flex-col gap-1"
      onSubmit={(e) => {
        e.preventDefault()
        onSubmit()
      }}
    >
      <div className="flex min-w-0 items-center justify-between gap-2">
        <p className="truncate text-[11px] font-medium text-[var(--color-fg)]">
          {pendingDoc ? pendingDocLabel(pendingDoc) : 'Could not analyze'}
        </p>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          disabled={busy}
          onClick={onCancel}
          className="h-6 shrink-0 px-1.5 text-[11px]"
        >
          Cancel
        </Button>
      </div>
      {scanNotes ? (
        <p className="truncate text-[10px] text-[var(--color-muted)]">{scanNotes}</p>
      ) : null}
      <div className="flex items-center gap-1">
        <Input
          inputMode="decimal"
          value={amount}
          onChange={(e) => setAmount(e.target.value)}
          className="h-7 w-20 shrink-0 px-2 text-xs tabular-nums"
          aria-label={`Amount (${ccy})`}
          required
          disabled={formDisabled}
        />
        <div className="w-[5.5rem] shrink-0 [&_input]:h-7 [&_input]:px-1.5 [&_input]:pr-7 [&_input]:text-[11px] [&_button]:h-6 [&_button]:w-6">
          <DateInput value={date} onChange={setDate} required disabled={formDisabled} />
        </div>
        {kind === 'bill' ? (
          <select
            className={cn(compactControl, 'h-7 w-[4.5rem] shrink-0')}
            value={billStatus}
            onChange={(e) => setBillStatus(e.target.value as BillStatusTray)}
            disabled={formDisabled}
          >
            <option value="unpaid">Unpaid</option>
            <option value="paid">Paid</option>
          </select>
        ) : null}
      </div>
      <div className="flex min-w-0 items-center gap-1">
        <select
          className={cn(compactControl, 'h-7 flex-1')}
          value={categoryId}
          onChange={(e) => setCategoryId(e.target.value)}
          disabled={formDisabled}
          aria-label="Category"
        >
          {categoryOpts.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
        {kind === 'bill' && billStatus === 'unpaid' ? (
          <select
            className={cn(compactControl, 'h-7 flex-1')}
            value={payableId}
            onChange={(e) => setPayableId(e.target.value)}
            disabled={formDisabled}
            aria-label="Payable"
          >
            {payableAccounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        ) : (
          <select
            className={cn(compactControl, 'h-7 flex-1')}
            value={walletId}
            onChange={(e) => setWalletId(e.target.value)}
            disabled={formDisabled}
            aria-label="Wallet"
          >
            {walletOpts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        )}
      </div>
      <div className="flex min-w-0 items-center gap-1">
        <Input
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Memo"
          className="h-7 flex-1 px-2 text-xs"
          disabled={formDisabled}
        />
        <Button
          type="submit"
          size="sm"
          busy={busy}
          disabled={formDisabled || !pendingDoc}
          className="h-7 shrink-0 px-2 text-xs"
        >
          Save
        </Button>
      </div>
    </form>
  )
}
