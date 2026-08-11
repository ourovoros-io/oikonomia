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
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  BookOpen,
  Check,
  FileText,
  Loader2,
  Receipt,
  Wallet,
} from 'lucide-react'
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
import { Button, Input } from '../components/ui'
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

const KIND_OPTIONS: Array<{
  id: EntryKind
  label: string
  short: string
  Icon: typeof ArrowUpRight
}> = [
  { id: 'expense', label: 'Expense', short: 'Out', Icon: ArrowUpRight },
  { id: 'income', label: 'Income', short: 'In', Icon: ArrowDownLeft },
  { id: 'bill', label: 'Bill', short: 'Bill', Icon: FileText },
  { id: 'transfer', label: 'Transfer', short: 'Move', Icon: ArrowLeftRight },
]

/** Quiet 32px controls — Vercel-tight, Oikonomia tokens. */
const ctl =
  'h-8 min-w-0 w-full rounded-md border border-[var(--color-border-strong)] bg-[var(--color-canvas)] px-2.5 text-[13px] text-[var(--color-fg)] outline-none transition placeholder:text-[var(--color-muted)] hover:border-[var(--color-muted)]/70 focus:border-[var(--color-fg)]/40 focus:bg-[var(--color-surface)] disabled:opacity-50'

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
  const [date] = useState(todayISO())
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
  const [analyzing, setAnalyzing] = useState(false)
  const [dragOver, setDragOver] = useState(false)
  const busyRef = useRef(false)
  const baseCurrencyRef = useRef('EUR')
  const analyzeGenRef = useRef(0)

  const reviewing = analyzing || pendingDoc != null

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
    void setQuickAddHeight(reviewing ? QUICK_ADD_REVIEW_HEIGHT : QUICK_ADD_IDLE_HEIGHT)
  }, [reviewing])

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
    if (loading || reviewing) return
    const t = window.setTimeout(() => {
      document.getElementById('quick-add-amount')?.focus()
    }, 30)
    return () => window.clearTimeout(t)
  }, [loading, reviewing])

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
    setError(null)
    if (!entity || !prefs) {
      applyRoleState(kindDefaultAccounts(next, accounts), roleSetters)
    } else {
      const key = lastAccountsMapKey(entity.id, next)
      const last = prefs.last_accounts_by_entity_kind[key]
      applyRoleState(resolveRoleAccounts(next, accounts, last), roleSetters)
    }
  }

  function applySuggestion(s: DocumentSuggestion, source: PendingDocSource) {
    setPendingDoc(source)
    setPendingAnalysis(JSON.stringify(s))

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
  }

  const processFile = useCallback(
    async (file: File, entId: string) => {
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
        setError((err as CommandError).message || 'Could not analyze')
        setPendingDoc(null)
        setPendingAnalysis(null)
      } finally {
        if (analyzeGenRef.current === gen) {
          busyRef.current = false
          setBusy(false)
          setAnalyzing(false)
        }
        setDragOver(false)
      }
    },
    [],
  )

  const processPath = useCallback(async (path: string, entId: string) => {
    if (busyRef.current) return
    const gen = analyzeGenRef.current + 1
    analyzeGenRef.current = gen
    busyRef.current = true
    setBusy(true)
    setAnalyzing(true)
    setError(null)
    try {
      const suggestion = await api.documentAnalyzePath({ entityId: entId, path })
      if (analyzeGenRef.current !== gen) return
      applySuggestion(suggestion, { kind: 'path', path })
    } catch (err) {
      if (analyzeGenRef.current !== gen) return
      setError((err as CommandError).message || 'Could not analyze')
      setPendingDoc(null)
      setPendingAnalysis(null)
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
            if (path) void processPath(path, entId)
            else setError('No file path received')
          }
        })
      } catch {
        // HTML5 fallback
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
    if (file && file.size > 0) void processFile(file, entityId)
  }

  function validateAccounts(): string | null {
    if ((kind === 'expense' || kind === 'income') && (!categoryId || !walletId)) {
      return 'Pick accounts'
    }
    if (kind === 'bill') {
      if (!categoryId) return 'Pick category'
      if (billStatus === 'paid' && !walletId) return 'Pick wallet'
      if (billStatus === 'unpaid' && !payableId) return 'Pick payable'
    }
    if (kind === 'transfer') {
      if (!fromId || !toId || fromId === toId) return 'Pick different accounts'
    }
    return null
  }

  async function onSubmit(ev?: FormEvent) {
    ev?.preventDefault()
    if (!entity || busy || analyzing) return
    const minor = parseMajorToMinor(amount, entity.base_currency)
    if (minor === null || minor <= 0) {
      setError('Invalid amount')
      return
    }
    const accountErr = validateAccounts()
    if (accountErr) {
      setError(accountErr)
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
        // ignore pref write failures
      }
      clearDocumentReview()
      setBusy(false)
      busyRef.current = false
      onBusyChange?.(false)
      onPosted({ kind, amountMinor: minor, currency: entity.base_currency })
    } catch (err) {
      setError((err as CommandError).message)
      setBusy(false)
      busyRef.current = false
    }
  }

  const formDisabled = busy || analyzing
  const ccy = entity?.base_currency ?? 'EUR'

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center gap-2 px-3 text-[13px] text-[var(--color-muted)]">
        <Loader2 className="size-3.5 animate-spin" aria-hidden />
        Loading
      </div>
    )
  }

  if (entities.length === 0) {
    return (
      <div className="flex h-full items-center gap-3 px-3">
        <BookOpen className="size-3.5 shrink-0 text-[var(--color-muted)]" strokeWidth={1.75} />
        <p className="min-w-0 flex-1 text-[13px] text-[var(--color-muted)]">Create a book first</p>
        <Button size="sm" className="h-8 shrink-0" onClick={() => void api.openMainWindow()}>
          Open
        </Button>
      </div>
    )
  }

  return (
    <form
      onSubmit={(e) => void onSubmit(e)}
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
        'relative flex h-full min-w-0 flex-col gap-2 overflow-hidden px-2.5 py-2 transition',
        dragOver && 'bg-[var(--color-accent-soft)]/40',
      )}
    >
      {/* Row 1: kind · amount · currency · Add */}
      <div className="flex min-h-8 shrink-0 items-stretch gap-1.5">
        <KindSegment value={kind} onChange={onKindChange} disabled={formDisabled} />
        <Input
          id="quick-add-amount"
          inputMode="decimal"
          placeholder="0.00"
          value={amount}
          onChange={(e) => setAmount(e.target.value)}
          className={cn(
            ctl,
            'h-8 min-h-8 flex-[1.4] text-[15px] font-medium tabular-nums tracking-tight',
          )}
          required
          disabled={formDisabled}
          aria-label={`Amount (${ccy})`}
        />
        <span className="inline-flex h-8 w-11 shrink-0 items-center justify-center rounded-md border border-[var(--color-border-strong)] bg-[var(--color-canvas)] text-[11px] font-medium text-[var(--color-muted)]">
          {ccy}
        </span>
        <Button
          type="submit"
          size="sm"
          busy={busy && !analyzing}
          disabled={formDisabled}
          className="h-8 shrink-0 px-3"
        >
          <Check className="size-3.5" strokeWidth={1.75} aria-hidden />
          Add
        </Button>
      </div>

      {/* Row 2: accounts · memo · optional entity / bill status */}
      <div className="flex min-h-8 shrink-0 items-stretch gap-1.5">
        {entities.length > 1 ? (
          <label className="relative flex w-[5.5rem] shrink-0">
            <span className="sr-only">Book</span>
            <select
              className={cn(ctl, 'ui-select h-8 cursor-pointer')}
              value={entityId ?? ''}
              disabled={formDisabled}
              onChange={(e) => void onEntityChange(e.target.value)}
              aria-label="Book"
            >
              {entities.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.name}
                </option>
              ))}
            </select>
          </label>
        ) : null}

        {kind === 'bill' ? (
          <select
            className={cn(ctl, 'ui-select h-8 w-[4.25rem] shrink-0 cursor-pointer')}
            value={billStatus}
            onChange={(e) => setBillStatus(e.target.value as BillStatusTray)}
            disabled={formDisabled}
            aria-label="Status"
          >
            <option value="unpaid">Due</option>
            <option value="paid">Paid</option>
          </select>
        ) : null}

        {(kind === 'expense' || kind === 'bill') && (
          <AccountSelect
            value={categoryId}
            onChange={setCategoryId}
            options={expenseAccounts}
            disabled={formDisabled}
            label="Category"
            icon={<Receipt className="size-3.5" strokeWidth={1.75} aria-hidden />}
          />
        )}
        {kind === 'income' && (
          <AccountSelect
            value={categoryId}
            onChange={setCategoryId}
            options={incomeAccounts}
            disabled={formDisabled}
            label="Income"
            icon={<ArrowDownLeft className="size-3.5" strokeWidth={1.75} aria-hidden />}
          />
        )}
        {(kind === 'expense' ||
          kind === 'income' ||
          (kind === 'bill' && billStatus === 'paid')) && (
          <AccountSelect
            value={walletId}
            onChange={setWalletId}
            options={kind === 'income' ? assetWallets : walletAccounts}
            disabled={formDisabled}
            label="Wallet"
            icon={<Wallet className="size-3.5" strokeWidth={1.75} aria-hidden />}
          />
        )}
        {kind === 'bill' && billStatus === 'unpaid' && (
          <AccountSelect
            value={payableId}
            onChange={setPayableId}
            options={payableAccounts}
            disabled={formDisabled}
            label="Payable"
            icon={<FileText className="size-3.5" strokeWidth={1.75} aria-hidden />}
          />
        )}
        {kind === 'transfer' && (
          <>
            <AccountSelect
              value={fromId}
              onChange={setFromId}
              options={transferAccounts}
              disabled={formDisabled}
              label="From"
              icon={<ArrowUpRight className="size-3.5" strokeWidth={1.75} aria-hidden />}
            />
            <AccountSelect
              value={toId}
              onChange={setToId}
              options={transferAccounts}
              disabled={formDisabled}
              label="To"
              icon={<ArrowDownLeft className="size-3.5" strokeWidth={1.75} aria-hidden />}
            />
          </>
        )}

        <Input
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Memo"
          className={cn(ctl, 'h-8 min-w-[5rem] flex-1')}
          disabled={formDisabled}
          aria-label="Memo"
        />
      </div>

      {/* Review strip when a document is pending */}
      {reviewing ? (
        <div className="flex min-h-8 shrink-0 items-center gap-2 rounded-md border border-[var(--color-border-strong)] bg-[var(--color-canvas)] px-2.5">
          {analyzing ? (
            <>
              <Loader2 className="size-3.5 shrink-0 animate-spin text-[var(--color-muted)]" />
              <p className="min-w-0 flex-1 truncate text-[12px] text-[var(--color-muted)]">
                Analyzing receipt…
              </p>
            </>
          ) : (
            <>
              <FileText
                className="size-3.5 shrink-0 text-[var(--color-muted)]"
                strokeWidth={1.75}
                aria-hidden
              />
              <p className="min-w-0 flex-1 truncate text-[12px] font-medium text-[var(--color-fg)]">
                {pendingDoc ? pendingDocLabel(pendingDoc) : 'Document'}
              </p>
            </>
          )}
          <Button
            type="button"
            size="sm"
            variant="secondary"
            className="h-7 shrink-0 px-2.5 text-[12px]"
            onClick={onCancelReview}
            disabled={busy && !analyzing}
          >
            Cancel
          </Button>
        </div>
      ) : (
        <p className="pointer-events-none absolute right-3 bottom-1.5 text-[10px] tracking-wide text-[var(--color-muted)]/70">
          Drop receipt
        </p>
      )}

      {error ? (
        <p
          className="pointer-events-none absolute inset-x-3 bottom-1 truncate text-center text-[11px] text-[var(--color-danger)]"
          role="alert"
        >
          {error}
        </p>
      ) : null}
    </form>
  )
}

function KindSegment({
  value,
  onChange,
  disabled,
}: {
  value: EntryKind
  onChange: (v: EntryKind) => void
  disabled: boolean
}) {
  return (
    <div
      className="inline-flex h-8 shrink-0 overflow-hidden rounded-md border border-[var(--color-border-strong)] bg-[var(--color-canvas)] p-0.5"
      role="radiogroup"
      aria-label="Entry type"
    >
      {KIND_OPTIONS.map((opt) => {
        const active = value === opt.id
        const Icon = opt.Icon
        return (
          <button
            key={opt.id}
            type="button"
            role="radio"
            aria-checked={active}
            disabled={disabled}
            title={opt.label}
            onClick={() => onChange(opt.id)}
            className={cn(
              'inline-flex h-full items-center gap-1 rounded-[5px] px-1.5 text-[11px] font-medium transition disabled:opacity-50',
              active
                ? 'bg-[var(--color-surface-2)] text-[var(--color-fg)]'
                : 'text-[var(--color-muted)] hover:text-[var(--color-fg-secondary)]',
            )}
          >
            <Icon className="size-3 shrink-0" strokeWidth={1.75} aria-hidden />
            <span className="hidden min-[420px]:inline">{opt.short}</span>
          </button>
        )
      })}
    </div>
  )
}

function AccountSelect({
  value,
  onChange,
  options,
  disabled,
  label,
  icon,
}: {
  value: string
  onChange: (v: string) => void
  options: AccountLike[]
  disabled: boolean
  label: string
  icon?: ReactNode
}) {
  return (
    <label className="relative flex min-h-0 min-w-0 flex-1">
      <span className="sr-only">{label}</span>
      {icon ? (
        <span className="pointer-events-none absolute top-1/2 left-2 z-[1] -translate-y-1/2 text-[var(--color-muted)]">
          {icon}
        </span>
      ) : null}
      <select
        className={cn(ctl, 'ui-select h-8 cursor-pointer', icon ? 'pl-7' : undefined)}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        disabled={disabled}
        required
        aria-label={label}
      >
        {options.length === 0 ? <option value="">—</option> : null}
        {options.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}
          </option>
        ))}
      </select>
    </label>
  )
}
