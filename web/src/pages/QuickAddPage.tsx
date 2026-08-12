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
import { CornerDownLeft, Loader2 } from 'lucide-react'
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
  QUICK_ADD_COMPACT_HEIGHT,
  QUICK_ADD_SAVE_HEIGHT,
  QUICK_ADD_STEPPER_HEIGHT,
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
  validateTrayAccounts,
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
  /** Cancel on Save step: hide panel + reset (same path as Esc). */
  onDismiss?: () => void
}

type Step = 'entity' | 'kind' | 'amount' | 'accounts' | 'save'

const KIND_OPTIONS: Array<{
  id: EntryKind
  label: string
}> = [
  { id: 'expense', label: 'Expense' },
  { id: 'income', label: 'Income' },
  { id: 'bill', label: 'Bill' },
  { id: 'transfer', label: 'Transfer' },
]

/** Quiet single-row control — fits 80px stepper chrome. */
const ctl =
  'h-8 min-w-0 w-full rounded-md border-0 bg-[var(--color-surface-2)] px-2 text-[13px] text-[var(--color-fg)] outline-none transition placeholder:text-[var(--color-muted)] focus:bg-[var(--color-canvas)] disabled:opacity-50'

const MAX_DOC_BYTES = 8 * 1024 * 1024
const ROLL_MS = 230

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

function Row({
  children,
  className,
  role,
  'aria-label': ariaLabel,
}: {
  children: ReactNode
  className?: string
  role?: string
  'aria-label'?: string
}) {
  return (
    <div
      role={role}
      aria-label={ariaLabel}
      className={cn('flex h-full min-w-0 items-center gap-2 overflow-hidden', className)}
    >
      {children}
    </div>
  )
}

export function QuickAddPage({ onPosted, onBusyChange, onDismiss }: Props) {
  const [entities, setEntities] = useState<Entity[]>([])
  const [entityId, setEntityId] = useState<string | null>(null)
  const [accounts, setAccounts] = useState<Account[]>([])
  const [prefs, setPrefs] = useState<UiPrefs | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const [step, setStep] = useState<Step>('kind')
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
  const amountRef = useRef<HTMLInputElement | null>(null)

  const entity = useMemo(
    () => entities.find((e) => e.id === entityId) ?? null,
    [entities, entityId],
  )

  baseCurrencyRef.current = entity?.base_currency ?? 'EUR'

  const roleSetters = useMemo(
    () => ({ setCategoryId, setWalletId, setPayableId, setFromId, setToId }),
    [],
  )

  const multiEntity = entities.length > 1
  const firstStep: Step = multiEntity ? 'entity' : 'kind'

  useEffect(() => {
    onBusyChange?.(busy || analyzing)
  }, [busy, analyzing, onBusyChange])

  // Height owner: compact empty books; save/confirm 120; analyze + rolls 80.
  useEffect(() => {
    if (loading) return
    if (entities.length === 0) {
      void setQuickAddHeight(QUICK_ADD_COMPACT_HEIGHT)
      return
    }
    if (analyzing) {
      void setQuickAddHeight(QUICK_ADD_STEPPER_HEIGHT)
      return
    }
    void setQuickAddHeight(step === 'save' ? QUICK_ADD_SAVE_HEIGHT : QUICK_ADD_STEPPER_HEIGHT)
  }, [loading, entities.length, analyzing, step])

  useEffect(() => {
    return () => {
      void setQuickAddHeight(QUICK_ADD_STEPPER_HEIGHT)
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
        // Book step only when multiple books exist.
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
      amountRef.current?.focus()
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
    if (step === 'save') {
      if (pendingDoc || analyzing) {
        clearDocumentReview()
      }
      rollTo('accounts', -1)
      return
    }
    if (step === 'kind' && multiEntity) {
      rollTo('entity', -1)
      return
    }
    if (step === 'amount') {
      rollTo('kind', -1)
      return
    }
    if (step === 'accounts') {
      rollTo('amount', -1)
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

  function onCancelAnalyze() {
    if (busy && !analyzing) return
    analyzeGenRef.current += 1
    busyRef.current = false
    setBusy(false)
    clearDocumentReview()
    setError(null)
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
      rollTo('save', 1)
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
      rollTo('save', 1)
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
      if (step !== 'amount') rollTo('amount', -1)
      return
    }
    const accountErr = validateTrayAccounts({
      kind,
      billStatus,
      categoryId,
      walletId,
      payableId,
      fromId,
      toId,
    })
    if (accountErr) {
      setError(accountErr)
      if (step === 'save') rollTo('accounts', -1)
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

  function advanceFromAmount() {
    if (!entity) return
    const minor = parseMajorToMinor(amount, entity.base_currency)
    if (minor === null || minor <= 0) {
      setError('Invalid amount')
      amountRef.current?.focus()
      return
    }
    rollTo('accounts', 1)
  }

  function advanceFromAccounts() {
    const accountErr = validateTrayAccounts({
      kind,
      billStatus,
      categoryId,
      walletId,
      payableId,
      fromId,
      toId,
    })
    if (accountErr) {
      setError(accountErr)
      return
    }
    rollTo('save', 1)
  }

  const formDisabled = busy || analyzing
  const ccy = entity?.base_currency ?? 'EUR'
  const showBack = step !== firstStep && !formDisabled

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
      <div className="flex h-full items-center gap-3 px-4">
        <p className="min-w-0 flex-1 text-[13px] text-[var(--color-fg-secondary)]">
          Create a book in Oikonomia first
        </p>
        <Button
          size="sm"
          className="h-8 shrink-0 rounded-full px-3.5"
          onClick={() => void api.openMainWindow()}
        >
          Open Oikonomia
        </Button>
      </div>
    )
  }

  function renderPanel(s: Step): ReactNode {
    if (s === 'save' && analyzing) {
      return (
        <Row>
          <Loader2 className="size-3.5 shrink-0 animate-spin text-[var(--color-muted)]" />
          <p className="min-w-0 flex-1 truncate text-[13px] text-[var(--color-muted)]">
            Analyzing…
          </p>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-8 shrink-0 rounded-full px-3 text-[12px]"
            onClick={onCancelAnalyze}
          >
            Cancel
          </Button>
        </Row>
      )
    }

    switch (s) {
      case 'entity':
        return (
          <Row>
            <span className="shrink-0 text-[12px] font-medium text-[var(--color-muted)]">
              Book
            </span>
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
                    'h-8 min-w-0 flex-1 truncate rounded-md px-2 text-[13px] font-medium transition disabled:opacity-50',
                    active
                      ? 'bg-[var(--color-accent-soft)] text-[var(--color-fg)]'
                      : 'bg-[var(--color-surface-2)] text-[var(--color-fg-secondary)] hover:text-[var(--color-fg)]',
                  )}
                >
                  {e.name}
                </button>
              )
            })}
            {entities.length > 3 ? (
              <label className="relative min-w-0 flex-1">
                <span className="sr-only">More books</span>
                <select
                  className={cn(ctl, 'ui-select h-8 cursor-pointer')}
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
          </Row>
        )

      case 'kind':
        return (
          <Row role="radiogroup" aria-label="Entry type">
            {KIND_OPTIONS.map((opt) => {
              const active = kind === opt.id
              return (
                <button
                  key={opt.id}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  disabled={formDisabled}
                  title={opt.label}
                  onClick={() => selectKind(opt.id)}
                  className={cn(
                    'h-8 min-w-0 flex-1 rounded-md px-2 text-[13px] font-medium transition disabled:opacity-50',
                    active
                      ? 'bg-[var(--color-accent-soft)] text-[var(--color-fg)]'
                      : 'bg-[var(--color-surface-2)] text-[var(--color-muted)] hover:text-[var(--color-fg-secondary)]',
                  )}
                >
                  {opt.label}
                </button>
              )
            })}
          </Row>
        )

      case 'amount':
        return (
          <form
            className="flex h-full min-w-0 flex-1 items-center gap-2 overflow-hidden"
            onSubmit={(e) => {
              e.preventDefault()
              advanceFromAmount()
            }}
          >
            <input
              ref={amountRef}
              id="quick-add-amount"
              inputMode="decimal"
              placeholder="0.00"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              className="h-10 min-w-0 flex-1 border-0 bg-transparent px-1 text-[23px] font-semibold tracking-tight text-[var(--color-fg)] tabular-nums outline-none placeholder:text-[var(--color-muted)]/55 disabled:opacity-50"
              required
              disabled={formDisabled}
              aria-label={`Amount (${ccy})`}
              autoComplete="off"
            />
            <span className="shrink-0 text-[12px] font-medium tracking-wide text-[var(--color-muted)]">
              {ccy}
            </span>
            <Button
              type="submit"
              size="sm"
              variant="ghost"
              disabled={formDisabled}
              aria-label="Next"
              className="h-8 w-8 shrink-0 rounded-full px-0 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              <CornerDownLeft className="size-3.5" strokeWidth={1.75} aria-hidden />
            </Button>
          </form>
        )

      case 'accounts':
        return (
          <form
            className="flex h-full min-w-0 flex-1 items-center gap-2 overflow-hidden"
            onSubmit={(e) => {
              e.preventDefault()
              advanceFromAccounts()
            }}
          >
            {(kind === 'expense' || kind === 'bill') && (
              <AccountSelect
                value={categoryId}
                onChange={setCategoryId}
                options={expenseAccounts}
                disabled={formDisabled}
                label="Category"
                prefix="Cat"
              />
            )}
            {kind === 'income' && (
              <AccountSelect
                value={categoryId}
                onChange={setCategoryId}
                options={incomeAccounts}
                disabled={formDisabled}
                label="Income"
                prefix="Inc"
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
                prefix="Wallet"
              />
            )}
            {kind === 'bill' && billStatus === 'unpaid' && (
              <AccountSelect
                value={payableId}
                onChange={setPayableId}
                options={payableAccounts}
                disabled={formDisabled}
                label="Payable"
                prefix="Payable"
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
                  prefix="From"
                />
                <AccountSelect
                  value={toId}
                  onChange={setToId}
                  options={transferAccounts}
                  disabled={formDisabled}
                  label="To"
                  prefix="To"
                />
              </>
            )}
            <Button
              type="submit"
              size="sm"
              variant="ghost"
              disabled={formDisabled}
              aria-label="Next"
              className="h-8 w-8 shrink-0 rounded-full px-0 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              <CornerDownLeft className="size-3.5" strokeWidth={1.75} aria-hidden />
            </Button>
          </form>
        )

      case 'save':
        return (
          <form
            className="flex h-full min-w-0 flex-1 flex-col justify-center gap-1.5 overflow-hidden"
            onSubmit={(e) => void onSubmit(e)}
          >
            <div className="flex min-w-0 items-center gap-2">
              {kind === 'bill' ? (
                <div
                  className="inline-flex h-7 shrink-0 items-center gap-0.5 rounded-md bg-[var(--color-surface-2)] p-0.5"
                  role="radiogroup"
                  aria-label="Bill status"
                >
                  {(
                    [
                      { id: 'unpaid', label: 'Due' },
                      { id: 'paid', label: 'Paid' },
                    ] as const
                  ).map((opt) => {
                    const active = billStatus === opt.id
                    return (
                      <button
                        key={opt.id}
                        type="button"
                        role="radio"
                        aria-checked={active}
                        disabled={formDisabled}
                        onClick={() => setBillStatus(opt.id)}
                        className={cn(
                          'h-full rounded px-2 text-[11px] font-medium transition disabled:opacity-50',
                          active
                            ? 'bg-[var(--color-accent-soft)] text-[var(--color-fg)]'
                            : 'text-[var(--color-muted)] hover:text-[var(--color-fg-secondary)]',
                        )}
                      >
                        {opt.label}
                      </button>
                    )
                  })}
                </div>
              ) : null}
              <Input
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Optional memo"
                className={cn(ctl, 'h-7 flex-1 shadow-none ring-0')}
                disabled={formDisabled}
                aria-label="Memo"
              />
              {pendingDoc ? (
                <span className="max-w-[7rem] shrink-0 truncate text-[11px] text-[var(--color-muted)]">
                  {pendingDocLabel(pendingDoc)}
                </span>
              ) : (
                <span className="shrink-0 text-[11px] text-[var(--color-muted)]/75">
                  Drop receipt
                </span>
              )}
            </div>
            <div className="flex min-w-0 items-center justify-end gap-2">
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className="h-8 shrink-0 rounded-full px-3 text-[12px]"
                disabled={busy && !analyzing}
                onClick={() => {
                  clearDocumentReview()
                  onDismiss?.()
                }}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                size="sm"
                busy={busy && !analyzing}
                disabled={formDisabled}
                className="h-8 shrink-0 rounded-full px-3.5 text-[12px]"
              >
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
        'relative flex h-full min-w-0 items-stretch overflow-hidden px-2.5 py-2 transition-colors duration-150',
        dragOver && 'bg-[var(--color-accent-soft)]/35',
      )}
    >
      <button
        type="button"
        onClick={goBack}
        disabled={!showBack}
        className={cn(
          'mr-0.5 inline-flex w-6 shrink-0 self-stretch items-center justify-center rounded-md text-[18px] leading-none transition',
          showBack
            ? 'text-[var(--color-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-fg)]'
            : 'pointer-events-none text-transparent',
        )}
        aria-label="Back"
        tabIndex={showBack ? 0 : -1}
      >
        ‹
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
          key={`enter-${step}-${analyzing ? 'a' : 'b'}`}
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
          className="pointer-events-none absolute inset-x-10 bottom-0.5 truncate text-center text-[11px] text-[var(--color-danger)]"
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
  prefix,
}: {
  value: string
  onChange: (v: string) => void
  options: AccountLike[]
  disabled: boolean
  label: string
  /** Quiet in-row role marker — visible, not sr-only. */
  prefix: string
}) {
  return (
    <label className="flex min-w-0 flex-1 items-center gap-1.5">
      <span className="shrink-0 text-[11px] font-medium text-[var(--color-muted)]">
        {prefix}
        <span className="mx-0.5 text-[var(--color-muted)]/55" aria-hidden>
          ·
        </span>
      </span>
      <select
        className={cn(ctl, 'ui-select h-8 min-w-0 flex-1 cursor-pointer')}
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
