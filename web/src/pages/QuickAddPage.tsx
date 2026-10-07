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
  type AccountDefaults,
  type DocumentSuggestion,
  type Entity,
  type LastRoleAccounts,
  type PendingDocSource,
  type UiPrefs,
} from '../lib/api'
import { bookCurrency, minorToInputText, parseMajorToMinor, type Currency } from '../lib/money'
import { fileToBase64, mimeFromName } from '../lib/files'
import {
  QUICK_ADD_COMPACT_HEIGHT,
  QUICK_ADD_SAVE_HEIGHT,
  QUICK_ADD_STEPPER_HEIGHT,
  setQuickAddHeight,
} from '../lib/quickAddWindow'
import { isTauri } from '../lib/tauri'
import { commandErrorMessage } from '../lib/commandError'
import { beginExclusive } from '../lib/guards'
import { HideFromExportControl } from '../components/hiddenUi'
import { Button } from '../components/ui'
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
import { useI18n } from '../lib/I18nProvider'

export type QuickAddPosted = {
  kind: EntryKind
  amountMinor: number
  currency: Currency
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
  labelKey: 'kind.expense' | 'kind.income' | 'kind.billShort' | 'kind.transferShort'
  titleKey: 'kind.expense' | 'kind.income' | 'kind.bill' | 'kind.transfer'
}> = [
  { id: 'expense', labelKey: 'kind.expense', titleKey: 'kind.expense' },
  { id: 'income', labelKey: 'kind.income', titleKey: 'kind.income' },
  { id: 'bill', labelKey: 'kind.billShort', titleKey: 'kind.bill' },
  { id: 'transfer', labelKey: 'kind.transferShort', titleKey: 'kind.transfer' },
]

/** Dense BUI v2 chrome for 300×64 tray — full-pill, hairline, h-7/h-8 rhythm. */
const ctlH = 'h-7'
const pillTrack =
  'flex h-8 w-full min-w-0 items-center gap-px rounded-full border border-[var(--color-border-strong)]/70 bg-[var(--color-canvas)]/90 p-0.5'
const pillField =
  'flex h-7 min-w-0 flex-1 items-center gap-1 rounded-full border border-[var(--color-border-strong)]/70 bg-[var(--color-surface-2)] px-1.5'
const pillChipQuiet =
  'inline-flex h-6 shrink-0 items-center rounded-full border border-[var(--color-border-strong)]/70 px-1.5 text-[11px] font-medium tracking-wide text-[var(--color-muted)]'
const nextCta =
  'inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--color-accent)] text-[#04140b] shadow-sm transition hover:bg-[var(--color-accent-hover)] disabled:opacity-50'
const saveCta =
  'h-7 shrink-0 rounded-full bg-[var(--color-accent)] px-2.5 text-[11px] font-medium text-[#04140b] shadow-[0_0_0_1px_rgba(53,176,107,0.35),0_0_16px_rgba(53,176,107,0.4)] transition hover:bg-[var(--color-accent-hover)] disabled:opacity-50'
const cancelCta =
  'h-7 shrink-0 rounded-full border border-[var(--color-border-strong)]/70 bg-[var(--color-surface-2)] px-2 text-[11px] font-medium text-[var(--color-fg-secondary)] transition hover:text-[var(--color-fg)] disabled:opacity-50'

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
  defaults: AccountDefaults | null,
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
  return kindDefaultAccounts(kind, defaults)
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

/** Single 80px row — shared vertical centerline + tight even gap for 300-wide. */
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
      className={cn(
        'flex h-full min-w-0 items-center gap-1.5 overflow-hidden',
        className,
      )}
    >
      {children}
    </div>
  )
}

function NextButton({ disabled, label }: { disabled: boolean; label: string }) {
  return (
    <button type="submit" disabled={disabled} aria-label={label} className={nextCta}>
      <CornerDownLeft className="size-3" strokeWidth={2} aria-hidden />
    </button>
  )
}

export function QuickAddPage({ onPosted, onBusyChange, onDismiss }: Props) {
  const { t } = useI18n()
  const [entities, setEntities] = useState<Entity[]>([])
  const [entityId, setEntityId] = useState<string | null>(null)
  const [accounts, setAccounts] = useState<Account[]>([])
  // Which account plays which role by default; Rust decides, this only holds it.
  const [defaults, setDefaults] = useState<AccountDefaults | null>(null)
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
  const [hidden, setHidden] = useState(false)
  const [busy, setBusy] = useState(false)

  const [pendingDoc, setPendingDoc] = useState<PendingDocSource | null>(null)
  const [pendingAnalysis, setPendingAnalysis] = useState<string | null>(null)
  const [analyzing, setAnalyzing] = useState(false)
  const [dragOver, setDragOver] = useState(false)
  const busyRef = useRef(false)
  const accountsGenRef = useRef(0)
  const bookCurrencyRef = useRef<Currency | null>(null)
  const analyzeGenRef = useRef(0)
  const amountRef = useRef<HTMLInputElement | null>(null)

  const entity = useMemo(
    () => entities.find((e) => e.id === entityId) ?? null,
    [entities, entityId],
  )

  bookCurrencyRef.current = entity ? bookCurrency(entity) : null

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
  const walletAccounts = useMemo(() => accountsOf(accounts, ['asset', 'liability']), [accounts])
  const assetWallets = useMemo(
    () => walletAccounts.filter((a) => a.account_type === 'asset'),
    [walletAccounts],
  )
  const payableAccounts = useMemo(() => accountsOf(accounts, ['liability']), [accounts])
  const transferAccounts = useMemo(() => accountsOf(accounts, ['asset']), [accounts])

  const loadAccountsFor = useCallback(
    async (entId: string, nextKind: EntryKind, uiPrefs: UiPrefs | null) => {
      const gen = ++accountsGenRef.current
      const [list, roles] = await Promise.all([api.accountList(entId), api.accountDefaults(entId)])
      if (gen !== accountsGenRef.current) return list
      setAccounts(list)
      setDefaults(roles)
      const key = lastAccountsMapKey(entId, nextKind)
      const last = uiPrefs?.last_accounts_by_entity_kind[key]
      applyRoleState(resolveRoleAccounts(nextKind, list, roles, last), roleSetters)
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
          setDefaults(null)
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
        if (!cancelled) setError(commandErrorMessage(err))
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
    const gen = accountsGenRef.current + 1
    setEntityId(nextId)
    setError(null)
    try {
      await loadAccountsFor(nextId, kind, prefs)
      if (gen !== accountsGenRef.current) return
      rollTo('kind', 1)
    } catch (err) {
      if (gen === accountsGenRef.current) setError(commandErrorMessage(err))
    }
  }

  function selectKind(next: EntryKind) {
    if (rollingRef.current) return
    setKind(next)
    if (next !== 'bill') setBillStatus('unpaid')
    if (!entity || !prefs) {
      applyRoleState(kindDefaultAccounts(next, defaults), roleSetters)
    } else {
      const key = lastAccountsMapKey(entity.id, next)
      const last = prefs.last_accounts_by_entity_kind[key]
      applyRoleState(resolveRoleAccounts(next, accounts, defaults, last), roleSetters)
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

    // Core leaves the amount out for a book whose currency the reader cannot
    // count in, so an amount that arrives is in the book's minor units.
    const currency = bookCurrencyRef.current
    if (currency && s.amount_minor != null && s.amount_minor > 0) {
      setAmount(minorToInputText(s.amount_minor, currency))
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
        setError(t('quickAdd.fileTooLarge'))
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
        setError(commandErrorMessage(err, 'quickAdd.couldNotAnalyze'))
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
    [rollTo, t],
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
        setError(commandErrorMessage(err, 'quickAdd.couldNotAnalyze'))
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
            else setError(t('quickAdd.noFilePath'))
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
    if (!entity || analyzing) return
    if (!beginExclusive(busyRef)) return
    const minor = parseMajorToMinor(amount, bookCurrency(entity))
    if (minor === null || minor <= 0) {
      busyRef.current = false
      setError(t('quickAdd.invalidAmount'))
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
      busyRef.current = false
      setError(accountErr)
      if (step === 'save') rollTo('accounts', -1)
      return
    }
    setBusy(true)
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
      const posted =
        pendingDoc?.kind === 'file'
          ? await api.entryPostSimpleWithDocument(
              input,
              {
                filename: pendingDoc.file.name,
                mimeType: pendingDoc.file.type || mimeFromName(pendingDoc.file.name),
                dataBase64: await fileToBase64(pendingDoc.file),
              },
              pendingAnalysis ?? undefined,
            )
          : pendingDoc?.kind === 'path'
            ? await api.entryPostSimpleWithDocumentPath(
                input,
                pendingDoc.path,
                pendingAnalysis ?? undefined,
              )
            : await api.entryPostSimple(input)
      if (hidden) {
        try {
          await api.entrySetHidden(posted.entry.id, true)
        } catch {
          // Retry hide. Do not void: the posted row stays; a second failure
          // surfaces on the existing save error alert.
          await api.entrySetHidden(posted.entry.id, true)
        }
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
      setHidden(false)
      setBusy(false)
      busyRef.current = false
      onBusyChange?.(false)
      onPosted({ kind, amountMinor: minor, currency: bookCurrency(entity) })
    } catch (err) {
      setError(commandErrorMessage(err))
      setBusy(false)
      busyRef.current = false
    }
  }

  function advanceFromAmount() {
    if (!entity) return
    const minor = parseMajorToMinor(amount, bookCurrency(entity))
    if (minor === null || minor <= 0) {
      setError(t('quickAdd.invalidAmount'))
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
      <div className="flex h-full items-center justify-center gap-1 px-1.5 text-[11px] text-[var(--color-muted)]">
        <Loader2 className="size-3 animate-spin" aria-hidden />
        {t('quickAdd.loading')}
      </div>
    )
  }

  if (entities.length === 0) {
    return (
      <div className="flex h-full items-center gap-1.5 px-2">
        <p className="min-w-0 flex-1 text-[11px] leading-snug text-[var(--color-fg-secondary)]">
          {t('quickAdd.createBookFirst')}
        </p>
        <Button
          size="sm"
          className="h-7 shrink-0 rounded-full px-2.5 text-[11px]"
          onClick={() => void api.openMainWindow()}
        >
          {t('common.open')}
        </Button>
      </div>
    )
  }

  function renderPanel(s: Step): ReactNode {
    if (s === 'save' && analyzing) {
      return (
        <Row>
          <Loader2 className="size-3 shrink-0 animate-spin text-[var(--color-muted)]" />
          <p className="min-w-0 flex-1 truncate text-[11px] leading-none text-[var(--color-muted)]">
            {t('quickAdd.analyzing')}
          </p>
          <button type="button" className={cancelCta} onClick={onCancelAnalyze}>
            {t('common.cancel')}
          </button>
        </Row>
      )
    }

    switch (s) {
      case 'entity':
        return (
          <Row>
            <div className={pillTrack} role="group" aria-label={t('quickAdd.book')}>
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
                      'flex h-full min-w-0 flex-1 items-center justify-center truncate rounded-full px-0.5 text-[11px] font-medium leading-none transition disabled:opacity-50',
                      active
                        ? 'bg-[var(--color-accent-soft)] text-[var(--color-fg)] ring-1 ring-inset ring-[var(--color-accent)]/45'
                        : 'text-[var(--color-muted)] hover:text-[var(--color-fg-secondary)]',
                    )}
                  >
                    {e.name}
                  </button>
                )
              })}
              {entities.length > 3 ? (
                <label className="relative flex h-full min-w-0 flex-1 items-center">
                  <span className="sr-only">{t('quickAdd.moreBooks')}</span>
                  <select
                    className="ui-select h-full w-full min-w-0 cursor-pointer rounded-full border-0 bg-transparent px-0.5 text-[11px] leading-none text-[var(--color-muted)] outline-none"
                    value={
                      entityId && entities.slice(3).some((e) => e.id === entityId) ? entityId : ''
                    }
                    disabled={formDisabled}
                    onChange={(e) => {
                      if (e.target.value) void selectEntity(e.target.value)
                    }}
                  >
                    <option value="">{t('quickAdd.more')}</option>
                    {entities.slice(3).map((e) => (
                      <option key={e.id} value={e.id}>
                        {e.name}
                      </option>
                    ))}
                  </select>
                </label>
              ) : null}
            </div>
          </Row>
        )

      case 'kind':
        return (
          <Row>
            <div className={pillTrack} role="radiogroup" aria-label={t('quickAdd.entryType')}>
              {KIND_OPTIONS.map((opt) => {
                const active = kind === opt.id
                return (
                  <button
                    key={opt.id}
                    type="button"
                    role="radio"
                    aria-checked={active}
                    disabled={formDisabled}
                    title={t(opt.titleKey)}
                    onClick={() => selectKind(opt.id)}
                    className={cn(
                      'flex h-full min-w-0 flex-1 items-center justify-center truncate rounded-full px-0.5 text-[11px] font-medium leading-none transition disabled:opacity-50',
                      active
                        ? 'bg-[var(--color-accent-soft)] text-[var(--color-fg)] ring-1 ring-inset ring-[var(--color-accent)]/45'
                        : 'text-[var(--color-muted)] hover:text-[var(--color-fg-secondary)]',
                    )}
                  >
                    {t(opt.labelKey)}
                  </button>
                )
              })}
            </div>
          </Row>
        )

      case 'amount':
        return (
          <form
            className="flex h-full min-w-0 flex-1 items-center gap-1.5 overflow-hidden"
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
              className={cn(
                ctlH,
                'min-w-0 flex-1 border-0 bg-transparent px-0.5 text-[17px] font-semibold leading-none tracking-tight text-[var(--color-fg)] tabular-nums outline-none placeholder:text-[var(--color-muted)]/55 disabled:opacity-50',
              )}
              required
              disabled={formDisabled}
              aria-label={t('quickAdd.amount', { ccy })}
              autoComplete="off"
            />
            <span className={pillChipQuiet}>{ccy}</span>
            <NextButton disabled={formDisabled} label={t('common.next')} />
          </form>
        )

      case 'accounts':
        return (
          <form
            className="flex h-full min-w-0 flex-1 items-center gap-1.5 overflow-hidden"
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
                label={t('quickAdd.category')}
                prefix={t('quickAdd.catPrefix')}
              />
            )}
            {kind === 'income' && (
              <AccountSelect
                value={categoryId}
                onChange={setCategoryId}
                options={incomeAccounts}
                disabled={formDisabled}
                label={t('quickAdd.income')}
                prefix={t('quickAdd.incPrefix')}
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
                label={t('quickAdd.wallet')}
                prefix={t('quickAdd.walletPrefix')}
              />
            )}
            {kind === 'bill' && billStatus === 'unpaid' && (
              <AccountSelect
                value={payableId}
                onChange={setPayableId}
                options={payableAccounts}
                disabled={formDisabled}
                label={t('quickAdd.payable')}
                prefix={t('quickAdd.payablePrefix')}
              />
            )}
            {kind === 'transfer' && (
              <>
                <AccountSelect
                  value={fromId}
                  onChange={setFromId}
                  options={transferAccounts}
                  disabled={formDisabled}
                  label={t('quickAdd.from')}
                  prefix={t('quickAdd.fromPrefix')}
                />
                <AccountSelect
                  value={toId}
                  onChange={setToId}
                  options={transferAccounts}
                  disabled={formDisabled}
                  label={t('quickAdd.to')}
                  prefix={t('quickAdd.toPrefix')}
                />
              </>
            )}
            <NextButton disabled={formDisabled} label={t('common.next')} />
          </form>
        )

      case 'save':
        return (
          <form
            className="flex h-full min-w-0 flex-1 flex-col justify-center gap-1 overflow-hidden"
            onSubmit={(e) => void onSubmit(e)}
          >
            <div className="flex h-7 min-w-0 items-center gap-1">
              {kind === 'bill' ? (
                <div
                  className="inline-flex h-7 shrink-0 items-center gap-px rounded-full border border-[var(--color-border-strong)]/70 bg-[var(--color-canvas)]/90 p-0.5"
                  role="radiogroup"
                  aria-label={t('quickAdd.billStatus')}
                >
                  {(
                    [
                      { id: 'unpaid', labelKey: 'quickAdd.due' as const },
                      { id: 'paid', labelKey: 'quickAdd.paid' as const },
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
                          'flex h-full items-center rounded-full px-1.5 text-[11px] font-medium leading-none transition disabled:opacity-50',
                          active
                            ? 'bg-[var(--color-accent-soft)] text-[var(--color-fg)] ring-1 ring-inset ring-[var(--color-accent)]/45'
                            : 'text-[var(--color-muted)] hover:text-[var(--color-fg-secondary)]',
                        )}
                      >
                        {t(opt.labelKey)}
                      </button>
                    )
                  })}
                </div>
              ) : null}
              <input
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder={t('quickAdd.memo')}
                className={cn(
                  pillField,
                  'min-w-0 text-[11px] leading-none outline-none placeholder:text-[var(--color-muted)] disabled:opacity-50',
                )}
                disabled={formDisabled}
                aria-label={t('quickAdd.memo')}
              />
              {pendingDoc ? (
                <span className="max-w-[3.75rem] shrink-0 truncate text-[11px] leading-none text-[var(--color-muted)]">
                  {pendingDocLabel(pendingDoc)}
                </span>
              ) : (
                <span className="shrink-0 text-[11px] leading-none text-[var(--color-muted)]/75">
                  {t('quickAdd.drop')}
                </span>
              )}
            </div>
            <HideFromExportControl
              compact
              checked={hidden}
              disabled={formDisabled}
              onChange={setHidden}
            />
            <div className="flex h-7 min-w-0 items-center justify-end gap-1">
              <button
                type="button"
                className={cancelCta}
                disabled={busy && !analyzing}
                onClick={() => {
                  clearDocumentReview()
                  onDismiss?.()
                }}
              >
                {t('common.cancel')}
              </button>
              <button
                type="submit"
                disabled={formDisabled || (busy && !analyzing)}
                className={cn(saveCta, 'inline-flex items-center justify-center gap-1')}
              >
                {busy && !analyzing ? (
                  <Loader2 className="size-3 shrink-0 animate-spin" aria-hidden />
                ) : null}
                {t('common.save')}
              </button>
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
        'relative flex h-full min-w-0 items-center gap-1 overflow-hidden px-1.5 transition-colors duration-150',
        dragOver && 'bg-[var(--color-accent-soft)]/35',
      )}
    >
      <button
        type="button"
        onClick={goBack}
        disabled={!showBack}
        className={cn(
          'inline-flex h-7 w-6 shrink-0 items-center justify-center rounded-full text-[14px] leading-none transition',
          showBack
            ? 'text-[var(--color-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-fg)]'
            : 'pointer-events-none text-transparent',
        )}
        aria-label={t('common.back')}
        tabIndex={showBack ? 0 : -1}
      >
        ‹
      </button>

      <div className="relative h-full min-h-0 min-w-0 flex-1 overflow-hidden">
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
          className="pointer-events-none absolute inset-x-6 bottom-1 truncate text-center text-[11px] leading-none text-[var(--color-danger)]"
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
  /** Quiet in-row role marker inside the pill field. */
  prefix: string
}) {
  return (
    <label className={cn(pillField, 'cursor-pointer')}>
      <span className="shrink-0 text-[11px] font-medium leading-none text-[var(--color-muted)]">
        {prefix}
      </span>
      <select
        className="ui-select h-6 min-w-0 flex-1 cursor-pointer border-0 bg-transparent p-0 text-[11px] leading-none text-[var(--color-fg)] outline-none disabled:opacity-50"
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
