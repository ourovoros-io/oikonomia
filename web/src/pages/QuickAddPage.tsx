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
  ChevronLeft,
  ChevronRight,
  FileText,
  Loader2,
  MessageSquare,
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
import { QUICK_ADD_IDLE_HEIGHT, setQuickAddHeight } from '../lib/quickAddWindow'
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

type Step = 'entity' | 'kind' | 'amount' | 'accounts' | 'memo' | 'review'

const KIND_OPTIONS: Array<{
  id: EntryKind
  label: string
  Icon: typeof ArrowUpRight
}> = [
  { id: 'expense', label: 'Expense', Icon: ArrowUpRight },
  { id: 'income', label: 'Income', Icon: ArrowDownLeft },
  { id: 'bill', label: 'Bill', Icon: FileText },
  { id: 'transfer', label: 'Transfer', Icon: ArrowLeftRight },
]

/** Controls fill the two-row strip; no horizontal scroll. */
const ctl =
  'h-8 min-w-0 w-full rounded-md border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] px-2 text-[11px] leading-none text-[var(--color-fg)] outline-none transition placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)] focus:ring-1 focus:ring-[var(--color-accent)]/25 disabled:opacity-50'

const choiceBtn =
  'flex min-h-0 min-w-0 flex-1 flex-col items-center justify-center gap-0.5 rounded-lg border px-1 py-1 text-center transition disabled:opacity-50'

const MAX_DOC_BYTES = 8 * 1024 * 1024
const ROLL_MS = 220

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

/** Full-height panel body: title row + control row that fills remaining space. */
function Panel({
  icon,
  title,
  children,
  className,
}: {
  icon: ReactNode
  title: string
  children: ReactNode
  className?: string
}) {
  return (
    <div className={cn('flex h-full min-w-0 flex-col gap-1 overflow-hidden py-0.5', className)}>
      <div className="flex h-5 shrink-0 items-center gap-1.5 px-0.5">
        <span className="inline-flex size-4 shrink-0 items-center justify-center text-[var(--color-accent)]">
          {icon}
        </span>
        <span className="truncate text-[11px] font-semibold tracking-wide text-[var(--color-fg)]">
          {title}
        </span>
      </div>
      <div className="flex min-h-0 min-w-0 flex-1 items-stretch gap-1 overflow-hidden">
        {children}
      </div>
    </div>
  )
}

export function QuickAddPage({ onPosted, onBusyChange }: Props) {
  const [entities, setEntities] = useState<Entity[]>([])
  const [entityId, setEntityId] = useState<string | null>(null)
  const [accounts, setAccounts] = useState<Account[]>([])
  const [prefs, setPrefs] = useState<UiPrefs | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const [step, setStep] = useState<Step>('entity')
  const [leaving, setLeaving] = useState<Step | null>(null)
  const [rollDir, setRollDir] = useState<1 | -1>(1)
  const rollingRef = useRef(false)

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

  async function onSubmit(ev?: FormEvent) {
    ev?.preventDefault()
    if (!entity || busy || analyzing) return
    const minor = parseMajorToMinor(amount, entity.base_currency)
    if (minor === null || minor <= 0) {
      setError('Invalid amount')
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
      onPosted({ kind, amountMinor: minor, currency: entity.base_currency })
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
      setError('Invalid amount')
      return
    }
    rollTo('accounts', 1)
  }

  function advanceFromAccounts() {
    if ((kind === 'expense' || kind === 'income') && (!categoryId || !walletId)) {
      setError('Pick accounts')
      return
    }
    if (kind === 'bill') {
      if (!categoryId) {
        setError('Pick category')
        return
      }
      if (billStatus === 'paid' && !walletId) {
        setError('Pick wallet')
        return
      }
      if (billStatus === 'unpaid' && !payableId) {
        setError('Pick payable')
        return
      }
    }
    if (kind === 'transfer') {
      if (!fromId || !toId || fromId === toId) {
        setError('Pick different accounts')
        return
      }
    }
    rollTo('memo', 1)
  }

  const formDisabled = busy || analyzing
  const ccy = entity?.base_currency ?? 'EUR'
  const showBack = step !== 'entity' && !formDisabled

  if (loading) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-1 text-[11px] text-[var(--color-muted)]">
        <Loader2 className="size-4 animate-spin text-[var(--color-accent)]" aria-hidden />
        Loading…
      </div>
    )
  }

  if (entities.length === 0) {
    return (
      <Panel icon={<BookOpen className="size-3.5" aria-hidden />} title="Book">
        <p className="flex min-w-0 flex-1 items-center truncate text-[11px] text-[var(--color-muted)]">
          Create a book first
        </p>
        <Button
          size="sm"
          className="h-8 shrink-0 px-2.5 text-[11px]"
          onClick={() => void api.openMainWindow()}
        >
          Open
        </Button>
      </Panel>
    )
  }

  function renderPanel(s: Step): ReactNode {
    switch (s) {
      case 'entity':
        return (
          <Panel icon={<BookOpen className="size-3.5" aria-hidden />} title="Choose book">
            {entities.slice(0, 3).map((e) => {
              const active = e.id === entityId
              return (
                <button
                  key={e.id}
                  type="button"
                  disabled={formDisabled}
                  onClick={() => void selectEntity(e.id)}
                  title={e.name}
                  className={cn(
                    choiceBtn,
                    active
                      ? 'border-[var(--color-accent)] bg-[var(--color-accent-soft)]'
                      : 'border-[var(--color-border-strong)] bg-[var(--color-surface-2)] hover:border-[var(--color-accent)]/50',
                  )}
                >
                  <BookOpen
                    className={cn(
                      'size-3.5 shrink-0',
                      active ? 'text-[var(--color-accent)]' : 'text-[var(--color-muted)]',
                    )}
                    aria-hidden
                  />
                  <span className="w-full truncate text-[10px] font-semibold text-[var(--color-fg)]">
                    {e.name}
                  </span>
                </button>
              )
            })}
            {entities.length > 3 ? (
              <label className="relative flex min-h-0 min-w-0 flex-1 flex-col">
                <span className="sr-only">More books</span>
                <select
                  className={cn(ctl, 'h-full min-h-8')}
                  value={
                    entityId && entities.slice(3).some((e) => e.id === entityId) ? entityId : ''
                  }
                  disabled={formDisabled}
                  onChange={(e) => {
                    if (e.target.value) void selectEntity(e.target.value)
                  }}
                >
                  <option value="">More…</option>
                  {entities.slice(3).map((e) => (
                    <option key={e.id} value={e.id}>
                      {e.name}
                    </option>
                  ))}
                </select>
              </label>
            ) : null}
          </Panel>
        )

      case 'kind':
        return (
          <Panel icon={<Receipt className="size-3.5" aria-hidden />} title="Entry type">
            {KIND_OPTIONS.map((opt) => {
              const active = kind === opt.id
              const Icon = opt.Icon
              return (
                <button
                  key={opt.id}
                  type="button"
                  disabled={formDisabled}
                  onClick={() => selectKind(opt.id)}
                  className={cn(
                    choiceBtn,
                    active
                      ? 'border-[var(--color-accent)] bg-[var(--color-accent-soft)]'
                      : 'border-[var(--color-border-strong)] bg-[var(--color-surface-2)] hover:border-[var(--color-accent)]/50',
                  )}
                >
                  <Icon
                    className={cn(
                      'size-3.5 shrink-0',
                      active ? 'text-[var(--color-accent)]' : 'text-[var(--color-muted)]',
                    )}
                    aria-hidden
                  />
                  <span className="w-full truncate text-[10px] font-semibold text-[var(--color-fg)]">
                    {opt.label}
                  </span>
                </button>
              )
            })}
          </Panel>
        )

      case 'amount':
        return (
          <form
            className="flex h-full min-w-0 flex-col gap-1 overflow-hidden py-0.5"
            onSubmit={(e) => {
              e.preventDefault()
              advanceFromAmount()
            }}
          >
            <div className="flex h-5 shrink-0 items-center gap-1.5 px-0.5">
              <Wallet className="size-3.5 shrink-0 text-[var(--color-accent)]" aria-hidden />
              <span className="truncate text-[11px] font-semibold text-[var(--color-fg)]">
                Amount
              </span>
            </div>
            <div className="flex min-h-0 min-w-0 flex-1 items-stretch gap-1">
              <Input
                id="quick-add-amount"
                inputMode="decimal"
                placeholder="0.00"
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
                className="h-full min-h-8 min-w-0 flex-[2] px-2 text-sm tabular-nums"
                required
                disabled={formDisabled}
                aria-label={`Amount (${ccy})`}
              />
              <span className="inline-flex h-full min-h-8 shrink-0 items-center rounded-md border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] px-2 text-[11px] font-semibold text-[var(--color-muted)]">
                {ccy}
              </span>
              <Button
                type="submit"
                size="sm"
                disabled={formDisabled}
                className="h-full min-h-8 shrink-0 gap-1 px-2.5 text-[11px]"
              >
                Next
                <ChevronRight className="size-3.5" aria-hidden />
              </Button>
            </div>
          </form>
        )

      case 'accounts':
        return (
          <form
            className="flex h-full min-w-0 flex-col gap-1 overflow-hidden py-0.5"
            onSubmit={(e) => {
              e.preventDefault()
              advanceFromAccounts()
            }}
          >
            <div className="flex h-5 shrink-0 items-center gap-1.5 px-0.5">
              <Wallet className="size-3.5 shrink-0 text-[var(--color-accent)]" aria-hidden />
              <span className="truncate text-[11px] font-semibold text-[var(--color-fg)]">
                Accounts
              </span>
            </div>
            <div className="flex min-h-0 min-w-0 flex-1 items-stretch gap-1">
              {kind === 'bill' ? (
                <select
                  className={cn(ctl, 'h-full min-h-8 w-[4rem] shrink-0')}
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
                  icon={<Receipt className="size-3" aria-hidden />}
                />
              )}
              {kind === 'income' && (
                <AccountSelect
                  value={categoryId}
                  onChange={setCategoryId}
                  options={incomeAccounts}
                  disabled={formDisabled}
                  label="Income"
                  icon={<ArrowDownLeft className="size-3" aria-hidden />}
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
                  icon={<Wallet className="size-3" aria-hidden />}
                />
              )}
              {kind === 'bill' && billStatus === 'unpaid' && (
                <AccountSelect
                  value={payableId}
                  onChange={setPayableId}
                  options={payableAccounts}
                  disabled={formDisabled}
                  label="Payable"
                  icon={<FileText className="size-3" aria-hidden />}
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
                    icon={<ArrowUpRight className="size-3" aria-hidden />}
                  />
                  <AccountSelect
                    value={toId}
                    onChange={setToId}
                    options={transferAccounts}
                    disabled={formDisabled}
                    label="To"
                    icon={<ArrowDownLeft className="size-3" aria-hidden />}
                  />
                </>
              )}
              <Button
                type="submit"
                size="sm"
                disabled={formDisabled}
                className="h-full min-h-8 shrink-0 gap-1 px-2.5 text-[11px]"
              >
                Next
                <ChevronRight className="size-3.5" aria-hidden />
              </Button>
            </div>
          </form>
        )

      case 'memo':
        return (
          <form
            className="flex h-full min-w-0 flex-col gap-1 overflow-hidden py-0.5"
            onSubmit={(e) => {
              e.preventDefault()
              void onSubmit()
            }}
          >
            <div className="flex h-5 shrink-0 items-center gap-1.5 px-0.5">
              <MessageSquare className="size-3.5 shrink-0 text-[var(--color-accent)]" aria-hidden />
              <span className="truncate text-[11px] font-semibold text-[var(--color-fg)]">Memo</span>
            </div>
            <div className="flex min-h-0 min-w-0 flex-1 items-stretch gap-1">
              <Input
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Optional note"
                className="h-full min-h-8 min-w-0 flex-1 px-2 text-[11px]"
                disabled={formDisabled}
              />
              <Button
                type="submit"
                size="sm"
                busy={busy}
                disabled={formDisabled}
                className="h-full min-h-8 shrink-0 gap-1 px-2.5 text-[11px]"
              >
                <Check className="size-3.5" aria-hidden />
                Save
              </Button>
            </div>
          </form>
        )

      case 'review':
        if (analyzing) {
          return (
            <Panel icon={<Loader2 className="size-3.5 animate-spin" aria-hidden />} title="Document">
              <p className="flex min-w-0 flex-1 items-center truncate text-[11px] text-[var(--color-muted)]">
                Analyzing…
              </p>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className="h-8 shrink-0 px-2 text-[11px]"
                onClick={onCancelReview}
              >
                Cancel
              </Button>
            </Panel>
          )
        }
        return (
          <form
            className="flex h-full min-w-0 flex-col gap-1 overflow-hidden py-0.5"
            onSubmit={(e) => {
              e.preventDefault()
              void onSubmit()
            }}
          >
            <div className="flex h-5 shrink-0 items-center gap-1.5 px-0.5">
              <FileText className="size-3.5 shrink-0 text-[var(--color-accent)]" aria-hidden />
              <span
                className="min-w-0 flex-1 truncate text-[11px] font-semibold text-[var(--color-fg)]"
                title={pendingDoc ? pendingDocLabel(pendingDoc) : undefined}
              >
                {pendingDoc ? pendingDocLabel(pendingDoc) : 'Failed'}
              </span>
              <button
                type="button"
                onClick={onCancelReview}
                className="shrink-0 text-[10px] font-medium text-[var(--color-muted)] hover:text-[var(--color-fg)]"
              >
                Clear
              </button>
            </div>
            <div className="flex min-h-0 min-w-0 flex-1 items-stretch gap-1">
              <Input
                inputMode="decimal"
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
                className="h-full min-h-8 w-[4.25rem] shrink-0 px-1.5 text-[11px] tabular-nums"
                disabled={formDisabled}
                required
                aria-label="Amount"
              />
              <AccountSelect
                value={categoryId}
                onChange={setCategoryId}
                options={kind === 'income' ? incomeAccounts : expenseAccounts}
                disabled={formDisabled}
                label="Category"
                icon={<Receipt className="size-3" aria-hidden />}
              />
              {kind === 'bill' && billStatus === 'unpaid' ? (
                <AccountSelect
                  value={payableId}
                  onChange={setPayableId}
                  options={payableAccounts}
                  disabled={formDisabled}
                  label="Payable"
                  icon={<FileText className="size-3" aria-hidden />}
                />
              ) : (
                <AccountSelect
                  value={walletId}
                  onChange={setWalletId}
                  options={kind === 'income' ? assetWallets : walletAccounts}
                  disabled={formDisabled}
                  label="Wallet"
                  icon={<Wallet className="size-3" aria-hidden />}
                />
              )}
              <Button
                type="submit"
                size="sm"
                busy={busy}
                disabled={formDisabled || !pendingDoc}
                className="h-full min-h-8 shrink-0 gap-1 px-2 text-[11px]"
              >
                <Check className="size-3.5" aria-hidden />
                Save
              </Button>
            </div>
          </form>
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
        'relative flex h-full min-w-0 items-stretch overflow-hidden px-1.5 py-1 transition',
        dragOver && 'bg-[var(--color-accent-soft)]/40',
      )}
    >
      <button
        type="button"
        onClick={goBack}
        disabled={!showBack}
        className={cn(
          'inline-flex w-7 shrink-0 self-stretch items-center justify-center rounded-md transition',
          showBack
            ? 'text-[var(--color-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-fg)]'
            : 'pointer-events-none text-transparent',
        )}
        aria-label="Back"
        tabIndex={showBack ? 0 : -1}
      >
        <ChevronLeft className="size-4" />
      </button>

      <div className="relative min-h-0 min-w-0 flex-1 overflow-hidden">
        {leaving ? (
          <div
            key={`leave-${leaving}`}
            className={cn(
              'absolute inset-0 overflow-hidden',
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
            'absolute inset-0 overflow-hidden',
            leaving ? (rollDir === 1 ? 'qa-enter-right' : 'qa-enter-left') : undefined,
          )}
        >
          {renderPanel(step)}
        </div>
      </div>

      {error ? (
        <p
          className="pointer-events-none absolute inset-x-8 bottom-0.5 truncate text-center text-[9px] text-[var(--color-danger)]"
          role="alert"
        >
          {error}
        </p>
      ) : null}
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
    <label className="relative flex min-h-0 min-w-0 flex-1 flex-col">
      <span className="sr-only">{label}</span>
      {icon ? (
        <span className="pointer-events-none absolute left-1.5 top-1/2 z-[1] -translate-y-1/2 text-[var(--color-muted)]">
          {icon}
        </span>
      ) : null}
      <select
        className={cn(ctl, 'h-full min-h-8', icon ? 'pl-6' : undefined)}
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
