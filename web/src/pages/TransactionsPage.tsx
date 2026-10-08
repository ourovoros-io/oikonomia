import { useEffect, useId, useMemo, useRef, useState, type FormEvent } from 'react'
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  FileText,
  Paperclip,
  Plus,
  Repeat,
  Trash2,
} from 'lucide-react'
import {
  api,
  formatDate,
  formatMoney,
  isoDate,
  todayISO,
  type Account,
  type AccountDefaults,
  type CashFlowSeries,
  type CsvImportPreview,
  type CsvColumnMapping,
  type DocumentMeta,
  type Entity,
  type LastRoleAccounts,
  type PendingDocSource,
  type PostedEntryView,
  type SimpleEntryInput,
} from '../lib/api'
import { parseMajorToMinor } from '../lib/amountParse'
import { bookCurrency, minorToInputText } from '../lib/money'
import { renderUiTexts, type UiText } from '../lib/uiText'
import { fileToBase64, mimeFromName } from '../lib/files'
import { beginExclusive } from '../lib/guards'
import { CashFlowPulse } from '../components/CashFlowPulse'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { CsvMappingModal } from '../components/CsvMappingModal'
import { CsvPreviewModal } from '../components/CsvPreviewModal'
import { DateInput } from '../components/DateInput'
import { DocumentDropZone } from '../components/DocumentDropZone'
import { DocumentViewerModal } from '../components/DocumentViewerModal'
import { EntryDetailModal } from '../components/EntryDetailModal'
import { HiddenBadge } from '../components/hiddenUi'
import { Modal } from '../components/Modal'
import { TopBar } from '../components/TopBar'
import { csvImportAccountDefaults, mappingsEqual } from '../lib/csvImport'
import { kindDefaultAccounts, lastAccountsMapKey } from '../lib/simpleEntry'
import {
  AmountPill,
  Button,
  EmptyState,
  ErrorBanner,
  Field,
  IconBadge,
  Input,
  MoneyPill,
  Panel,
  Segmented,
  Select,
} from '../components/ui'
import { cn } from '../lib/cn'
import { asCommandError, commandErrorMessage } from '../lib/commandError'
import type { CommandError } from '../lib/tauri'
import type { DocumentSuggestion } from '../lib/api'
import { formatMoney as fmtMoney } from '../lib/money'
import { useI18n } from '../lib/I18nProvider'
import { RecurringPage } from './RecurringPage'

type Props = {
  entity: Entity | null
  onCreateBook?: () => void
  /** Bumped by the sidebar's Quick add: open New entry once, then report it handled. */
  newEntryIntent?: number
  onNewEntryIntentHandled?: () => void
}

/** High-level entry kinds so users don't think in debit/credit. */
type EntryKind = 'expense' | 'income' | 'bill' | 'transfer'

type BillStatus = 'paid' | 'unpaid' | 'pay_existing'

function accountsOf(accounts: Account[], types: Account['account_type'][]): Account[] {
  return accounts.filter((a) => a.is_active && types.includes(a.account_type))
}

function inferKind(
  view: PostedEntryView,
  accountMap: Map<string, Account>,
): 'expense' | 'income' | 'transfer' | 'other' {
  const types = view.lines.map((l) => accountMap.get(l.account_id)?.account_type)
  if (types.includes('expense')) return 'expense'
  if (types.includes('income')) return 'income'
  if (types.every((t) => t === 'asset' || t === 'liability')) return 'transfer'
  return 'other'
}

export function TransactionsPage({
  entity,
  onCreateBook,
  newEntryIntent,
  onNewEntryIntentHandled,
}: Props) {
  const { t } = useI18n()
  const [entries, setEntries] = useState<PostedEntryView[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])
  // Which account plays which role by default; Rust decides, this only holds it.
  const [defaults, setDefaults] = useState<AccountDefaults | null>(null)
  const [error, setError] = useState<string | null>(null)
  const errorBannerId = useId()
  const [showForm, setShowForm] = useState(false)

  const [kind, setKind] = useState<EntryKind>('expense')
  const [billStatus, setBillStatus] = useState<BillStatus>('paid')
  const [date, setDate] = useState(todayISO())
  const [description, setDescription] = useState('')
  const [reference, setReference] = useState('')
  const [categoryId, setCategoryId] = useState('') // expense or income account
  const [walletId, setWalletId] = useState('') // bank / cash / card
  const [payableId, setPayableId] = useState('') // bills payable / AP
  const [fromId, setFromId] = useState('') // transfer
  const [toId, setToId] = useState('')
  const [amount, setAmount] = useState('')
  const [amountInvalid, setAmountInvalid] = useState(false)
  const [busy, setBusy] = useState(false)
  const [voidId, setVoidId] = useState<string | null>(null)
  const [voidBusy, setVoidBusy] = useState(false)
  const [editId, setEditId] = useState<string | null>(null)
  const [pendingDoc, setPendingDoc] = useState<PendingDocSource | null>(null)
  const [pendingAnalysis, setPendingAnalysis] = useState<string | null>(null)
  const [scanNotes, setScanNotes] = useState<UiText[] | null>(null)
  const [docs, setDocs] = useState<DocumentMeta[]>([])
  const [detailId, setDetailId] = useState<string | null>(null)
  const [viewerDocId, setViewerDocId] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const [fromDate, setFromDate] = useState('')
  const [toDate, setToDate] = useState('')
  const [accountFilter, setAccountFilter] = useState('')
  const [csvPreview, setCsvPreview] = useState<CsvImportPreview | null>(null)
  const [csvStep, setCsvStep] = useState<'closed' | 'mapping' | 'preview'>('closed')
  const [subview, setSubview] = useState<'journal' | 'recurring'>('journal')
  const [csvBusy, setCsvBusy] = useState<'import' | 'export' | 'post' | null>(null)
  const [csvRoles, setCsvRoles] = useState<{
    wallet_account_id: string | null
    expense_account_id: string | null
    income_account_id: string | null
  } | null>(null)
  const [series, setSeries] = useState<CashFlowSeries | null>(null)
  const [seriesError, setSeriesError] = useState<CommandError | null>(null)
  const summaryHeadingId = useId()
  const prevEntityId = useRef<string | null>(null)
  const busyRef = useRef(false)
  const csvBusyRef = useRef(false)
  const seriesRequestRef = useRef(0)

  const accountMap = useMemo(() => new Map(accounts.map((a) => [a.id, a])), [accounts])

  /** Hide voided pairs; the backend marks both sides via is_voided. */
  const visibleEntries = useMemo(() => entries.filter((e) => !e.is_voided), [entries])
  const postedCount = useMemo(
    () => visibleEntries.filter((e) => !e.entry.hidden).length,
    [visibleEntries],
  )
  const hiddenCount = useMemo(
    () => visibleEntries.filter((e) => e.entry.hidden).length,
    [visibleEntries],
  )

  // Debounce typing so each keystroke doesn't hit SQLite.
  useEffect(() => {
    const t = setTimeout(() => setDebouncedSearch(search), 300)
    return () => clearTimeout(t)
  }, [search])

  const docsByEntry = useMemo(() => {
    const map = new Map<string, DocumentMeta[]>()
    for (const d of docs) {
      const list = map.get(d.entry_id) ?? []
      list.push(d)
      map.set(d.entry_id, list)
    }
    return map
  }, [docs])

  const detailView = useMemo(
    () => visibleEntries.find((e) => e.entry.id === detailId) ?? null,
    [visibleEntries, detailId],
  )

  const filtersActive = Boolean(
    debouncedSearch.trim() || fromDate || toDate || accountFilter,
  )

  /** The summary follows dates only; these filters narrow the list, not it. */
  const filtersNarrowList = Boolean(debouncedSearch.trim() || accountFilter)

  const expenseAccounts = useMemo(() => accountsOf(accounts, ['expense']), [accounts])
  const incomeAccounts = useMemo(() => accountsOf(accounts, ['income']), [accounts])
  const walletAccounts = useMemo(() => accountsOf(accounts, ['asset', 'liability']), [accounts])
  const payableAccounts = useMemo(() => accountsOf(accounts, ['liability']), [accounts])

  function applyKindDefaults(nextKind: EntryKind, roles: AccountDefaults | null) {
    const picked = kindDefaultAccounts(nextKind, roles)

    if (nextKind === 'transfer') {
      setFromId(picked.fromId)
      setToId(picked.toId)
      return
    }

    setCategoryId(picked.categoryId)
    setWalletId(picked.walletId)
    if (nextKind === 'bill') {
      setPayableId(picked.payableId)
      setBillStatus('paid')
    }
  }

  /**
   * The summary is decorative next to the ledger list: it must never hold up
   * a reload, and a stale in-flight request must never clobber a fresher one.
   * Rust alone decides whether the from/to window is valid.
   */
  function loadSeries() {
    if (!entity) return
    const requestId = seriesRequestRef.current + 1
    seriesRequestRef.current = requestId
    api.cashFlowSeries(entity.id, fromDate || null, toDate || null).then(
      (flow) => {
        if (seriesRequestRef.current !== requestId) return
        setSeries(flow)
        setSeriesError(null)
      },
      (err) => {
        if (seriesRequestRef.current !== requestId) return
        setSeries(null)
        setSeriesError(asCommandError(err))
      },
    )
  }

  async function reload() {
    if (!entity) return
    const [e, a, d, roles] = await Promise.all([
      api.entryList(entity.id, {
        search: debouncedSearch.trim() || undefined,
        from: fromDate || undefined,
        to: toDate || undefined,
        accountId: accountFilter || undefined,
      }),
      api.accountList(entity.id),
      api.documentList(entity.id),
      api.accountDefaults(entity.id),
    ])
    setEntries(e)
    setAccounts(a)
    setDefaults(roles)
    setDocs(d)
    loadSeries()
    if (!categoryId && !walletId) {
      applyKindDefaults(kind, roles)
    }
  }

  function closeCsvFlow() {
    if (csvBusy === 'post' || csvBusy === 'import') return
    setCsvStep('closed')
    setCsvPreview(null)
    setCsvRoles(null)
  }

  async function lastAccountsForCsv(): Promise<{
    expenseLast?: LastRoleAccounts | null
    incomeLast?: LastRoleAccounts | null
  }> {
    if (!entity) return {}
    try {
      const prefs = await api.getUiPrefs()
      return {
        expenseLast: prefs.last_accounts_by_entity_kind[lastAccountsMapKey(entity.id, 'expense')],
        incomeLast: prefs.last_accounts_by_entity_kind[lastAccountsMapKey(entity.id, 'income')],
      }
    } catch {
      return {}
    }
  }

  async function onImportCsv() {
    if (!entity) return
    if (!beginExclusive(csvBusyRef)) return
    setCsvBusy('import')
    setError(null)
    try {
      const last = await lastAccountsForCsv()
      const csvDefaults = csvImportAccountDefaults({
        defaults,
        walletId,
        categoryId,
        kind,
        expenseLast: last.expenseLast,
        incomeLast: last.incomeLast,
      })
      const preview = await api.csvImportPreview({
        entity_id: entity.id,
        wallet_account_id: csvDefaults.wallet_account_id,
        expense_account_id: csvDefaults.expense_account_id,
        income_account_id: csvDefaults.income_account_id,
      })
      if (!preview) return
      setCsvRoles(csvDefaults)
      setCsvPreview(preview)
      setCsvStep('mapping')
    } catch (err) {
      setError(commandErrorMessage(err))
    } finally {
      csvBusyRef.current = false
      setCsvBusy(null)
    }
  }

  async function onMappingContinue(mapping: CsvColumnMapping, unchanged: boolean) {
    if (!entity || !csvPreview) return
    // A preview that names missing columns never takes this shortcut: its
    // detected mapping is incomplete, and the dialog only continues with a
    // complete one, which therefore differs from it.
    if (
      unchanged ||
      !csvPreview.source ||
      mappingsEqual(mapping, csvPreview.detected_mapping ?? {})
    ) {
      setCsvStep('preview')
      return
    }
    if (!beginExclusive(csvBusyRef)) return
    setCsvBusy('import')
    setError(null)
    try {
      const preview = await api.csvImportPreview({
        entity_id: entity.id,
        path: csvPreview.source,
        wallet_account_id: csvRoles?.wallet_account_id ?? null,
        expense_account_id: csvRoles?.expense_account_id ?? null,
        income_account_id: csvRoles?.income_account_id ?? null,
        mapping,
      })
      if (!preview) return
      setCsvPreview(preview)
      setCsvStep('preview')
    } catch (err) {
      setError(commandErrorMessage(err))
    } finally {
      csvBusyRef.current = false
      setCsvBusy(null)
    }
  }

  async function onExportCsv() {
    if (!entity) return
    if (!beginExclusive(csvBusyRef)) return
    setCsvBusy('export')
    setError(null)
    try {
      await api.csvExportJournal(entity.id)
    } catch (err) {
      setError(commandErrorMessage(err))
    } finally {
      csvBusyRef.current = false
      setCsvBusy(null)
    }
  }

  async function onCsvPost(input: { rows: SimpleEntryInput[]; include_duplicates: boolean }) {
    if (!beginExclusive(csvBusyRef)) return
    setCsvBusy('post')
    setError(null)
    try {
      await api.csvImportPost(input)
      setCsvStep('closed')
      setCsvPreview(null)
      setCsvRoles(null)
      await reload()
    } catch (err) {
      setError(commandErrorMessage(err))
    } finally {
      csvBusyRef.current = false
      setCsvBusy(null)
    }
  }

  /**
   * Blank new-entry baseline. Close and applySuggestion only write fields
   * the next source provides, so leftover description/amount/reference
   * would otherwise survive a sparse drop.
   */
  function resetDraft() {
    setKind('expense')
    setBillStatus('paid')
    setDate(todayISO())
    setDescription('')
    setReference('')
    setAmount('')
    setAmountInvalid(false)
    setPendingDoc(null)
    setPendingAnalysis(null)
    setScanNotes(null)
    setEditId(null)
    applyKindDefaults('expense', defaults)
  }

  function closeForm() {
    if (busy) return
    resetDraft()
    setShowForm(false)
  }

  function openNewEntry() {
    resetDraft()
    setShowForm(true)
  }

  useEffect(() => {
    if (!entity) {
      setEntries([])
      setAccounts([])
      setDefaults(null)
      setDocs([])
      prevEntityId.current = null
      return
    }
    if (prevEntityId.current !== entity.id) {
      prevEntityId.current = entity.id
      setDetailId(null)
      setViewerDocId(null)
      setCsvStep('closed')
      setCsvPreview(null)
      setCsvRoles(null)
      // Clear the un-debounced fragment too, so it cannot filter the new
      // book 300 ms later.
      setSearch('')
      setDebouncedSearch('')
      // Gate the early return on tracked deps only: resetting `search`
      // alone changes no dependency, and returning then would skip the
      // reload and leave the old book's entries on screen.
      if (debouncedSearch || fromDate || toDate || accountFilter) {
        setFromDate('')
        setToDate('')
        setAccountFilter('')
        return
      }
    }
    void reload().catch((err) => setError(commandErrorMessage(err)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity?.id, debouncedSearch, fromDate, toDate, accountFilter])

  // The sidebar's Quick add lands here. Reporting it handled lets App reset the
  // intent, so a later remount does not reopen the dialog.
  useEffect(() => {
    if (!entity || !newEntryIntent) return
    setSubview('journal')
    openNewEntry()
    onNewEntryIntentHandled?.()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [newEntryIntent, entity?.id])

  function setKindAndDefaults(next: EntryKind) {
    setKind(next)
    applyKindDefaults(next, defaults)
  }

  function applySuggestion(s: DocumentSuggestion, source: PendingDocSource) {
    resetDraft()
    setShowForm(true)
    setPendingDoc(source)
    setPendingAnalysis(JSON.stringify(s))
    setScanNotes(s.notes)

    if (s.kind === 'bill') {
      setKind('bill')
      applyKindDefaults('bill', defaults)
      setBillStatus(s.bill_unpaid ? 'unpaid' : 'paid')
    } else if (s.kind === 'income') {
      setKind('income')
      applyKindDefaults('income', defaults)
    } else {
      setKind('expense')
      applyKindDefaults('expense', defaults)
    }

    if (s.entry_date) setDate(s.entry_date)
    if (s.description) setDescription(s.description)
    else if (s.merchant) setDescription(s.merchant)
    if (s.reference) setReference(s.reference)
    // Core leaves the amount out for a book whose currency the reader cannot
    // count in, so an amount that arrives is in the book's minor units.
    if (entity && s.amount_minor != null && s.amount_minor > 0) {
      setAmount(minorToInputText(s.amount_minor, bookCurrency(entity)))
    }
    if (s.category_account_id) setCategoryId(s.category_account_id)
    if (s.wallet_account_id) setWalletId(s.wallet_account_id)
    if (s.payable_account_id) setPayableId(s.payable_account_id)
  }

  async function onPost(ev: FormEvent) {
    ev.preventDefault()
    if (!entity) return
    if (!beginExclusive(busyRef)) return
    const minor = parseMajorToMinor(amount, bookCurrency(entity))
    if (minor === null || minor <= 0) {
      busyRef.current = false
      setError(t('tx.invalidAmount'))
      setAmountInvalid(true)
      return
    }
    // Role/account rules live in Rust (post_simple_entry); its Validation
    // errors surface in the banner below.

    setBusy(true)
    setError(null)
    setAmountInvalid(false)
    try {
      const input = {
        entity_id: entity.id,
        kind,
        bill_status: kind === 'bill' ? billStatus : null,
        entry_date: date,
        description: description.trim(),
        reference: reference.trim() || null,
        amount_minor: minor,
        category_account_id: categoryId || null,
        wallet_account_id: walletId || null,
        payable_account_id: payableId || null,
        from_account_id: fromId || null,
        to_account_id: toId || null,
      }

      if (editId) {
        await api.entryReplaceSimple(editId, input)
      } else if (pendingDoc?.kind === 'file') {
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
        await api.entryPostSimpleWithDocumentPath(input, pendingDoc.path, pendingAnalysis ?? undefined)
      } else {
        await api.entryPostSimple(input)
      }
      resetDraft()
      setShowForm(false)
      await reload()
    } catch (err) {
      setError(commandErrorMessage(err))
    } finally {
      busyRef.current = false
      setBusy(false)
    }
  }

  async function confirmVoid() {
    if (!voidId) return
    const id = voidId
    setVoidBusy(true)
    setError(null)
    try {
      await api.entryVoid(id)
      setVoidId(null)
      // Drop from UI immediately so the row disappears even before reload finishes.
      setEntries((prev) => prev.filter((e) => e.entry.id !== id && !e.is_voided))
      await reload()
    } catch (err) {
      setError(commandErrorMessage(err, 'tx.deleteFailed'))
    } finally {
      setVoidBusy(false)
    }
  }

  if (!entity) {
    return (
      <EmptyState
        icon={<ArrowLeftRight className="size-5" />}
        title={t('tx.noBookTitle')}
        body={t('tx.noBookBody')}
        action={
          onCreateBook ? (
            <Button onClick={onCreateBook}>{t('empty.createBook')}</Button>
          ) : undefined
        }
      />
    )
  }

  const ccy = entity.base_currency
  const currency = bookCurrency(entity)

  if (subview === 'recurring') {
    return (
      <RecurringPage entity={entity} onBack={() => setSubview('journal')} />
    )
  }

  const csvActions = (
    <>
      <Button variant="secondary" size="sm" onClick={() => setSubview('recurring')}>
        <Repeat className="size-3" />
        {t('tx.recurring')}
      </Button>
      <Button
        variant="secondary"
        size="sm"
        busy={csvBusy === 'import'}
        disabled={csvBusy !== null && csvBusy !== 'import'}
        onClick={() => void onImportCsv()}
      >
        {t('tx.csv.import')}
      </Button>
      <Button
        variant="secondary"
        size="sm"
        busy={csvBusy === 'export'}
        disabled={csvBusy !== null && csvBusy !== 'export'}
        onClick={() => void onExportCsv()}
        title={t('tx.csv.exportTitle')}
      >
        {t('tx.csv.export')}
      </Button>
    </>
  )

  const newEntryButton = (
    <Button onClick={openNewEntry}>
      <Plus className="size-4" />
      {t('tx.newEntry')}
    </Button>
  )

  const netTone = !series || series.net_minor === 0 ? 'zero' : series.net_minor > 0 ? 'in' : 'out'
  const range = series ? `${formatDate(series.from)} – ${formatDate(series.to)}` : null

  /**
   * Map a posted entry's lines back onto the simple form and open it for
   * editing. Bills reopen as their expense/transfer equivalent — the journal
   * lines are identical, so nothing is lost.
   */
  function startEdit(view: PostedEntryView) {
    const debit = view.lines.find((l) => l.debit.amount_minor > 0)
    const credit = view.lines.find((l) => l.credit.amount_minor > 0)
    if (!debit || !credit) return

    resetDraft()

    const debitType = accountMap.get(debit.account_id)?.account_type
    const creditType = accountMap.get(credit.account_id)?.account_type
    if (debitType === 'expense') {
      setKind('expense')
      setCategoryId(debit.account_id)
      setWalletId(credit.account_id)
    } else if (creditType === 'income') {
      setKind('income')
      setCategoryId(credit.account_id)
      setWalletId(debit.account_id)
    } else {
      setKind('transfer')
      setToId(debit.account_id)
      setFromId(credit.account_id)
    }

    const amountMinor = view.lines.reduce((s, l) => s + l.debit.amount_minor, 0)
    setAmount(minorToInputText(amountMinor, currency))
    setDate(isoDate(view.entry.entry_date))
    setDescription(view.entry.description)
    setReference(view.entry.reference ?? '')
    setPendingDoc(null)
    setPendingAnalysis(null)
    setScanNotes(null)
    setEditId(view.entry.id)
    setDetailId(null)
    setShowForm(true)
  }

  return (
    <div className="space-y-4">
      <TopBar title={t('tx.title')} subtitle={`${entity.name} · ${ccy}`} actions={newEntryButton} />

      <ErrorBanner id={errorBannerId} message={error} />

      <CsvMappingModal
        open={csvStep === 'mapping'}
        preview={csvPreview}
        busy={csvBusy === 'import'}
        onClose={closeCsvFlow}
        onContinue={(mapping, unchanged) => void onMappingContinue(mapping, unchanged)}
      />

      <CsvPreviewModal
        open={csvStep === 'preview'}
        preview={csvPreview}
        currency={currency}
        walletAccounts={walletAccounts}
        expenseAccounts={expenseAccounts}
        incomeAccounts={incomeAccounts}
        busy={csvBusy === 'post'}
        onClose={closeCsvFlow}
        onConfirm={(input) => void onCsvPost(input)}
      />

      <ConfirmDialog
        open={voidId !== null}
        title={t('tx.deleteTitle')}
        body={t('tx.deleteBody')}
        confirmLabel={t('common.delete')}
        danger
        busy={voidBusy}
        onCancel={() => {
          if (!voidBusy) setVoidId(null)
        }}
        onConfirm={() => void confirmVoid()}
      />

      <div className="grid gap-4 lg:grid-cols-[minmax(0,1.75fr)_minmax(0,1fr)]">
        <section
          aria-labelledby={summaryHeadingId}
          className="glass-pane relative flex flex-col overflow-hidden rounded-[20px]"
        >
          <div className="flex flex-wrap items-start justify-between gap-4 px-5 pt-5">
            <div className="min-w-0">
              <h2
                id={summaryHeadingId}
                className="font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase"
              >
                {range ? t('tx.summary.inView', { range }) : t('tx.summary.inViewEmpty')}
              </h2>
              <p
                data-net={netTone}
                className={cn(
                  'mt-2 truncate text-[2rem] leading-none font-semibold tracking-[-0.01em] tabular-nums',
                  netTone === 'in' ? 'net-figure-in' : netTone === 'out' ? 'net-figure-out' : 'text-[var(--color-fg)]',
                )}
              >
                {series ? formatMoney(series.net_minor, currency, undefined, { signed: true }) : '—'}
              </p>
              {seriesError?.code === 'date_range_inverted' ? (
                <p className="mt-1.5 text-xs text-[var(--color-danger)]">{t('tx.summary.invalidRange')}</p>
              ) : seriesError ? (
                <p className="mt-1.5 text-xs text-[var(--color-danger)]">{commandErrorMessage(seriesError)}</p>
              ) : filtersNarrowList ? (
                <p className="mt-1.5 text-xs text-[var(--color-muted)]">{t('tx.summary.wholeBook')}</p>
              ) : null}
            </div>
            <div className="flex flex-wrap gap-2">
              <MoneyPill
                tone="in"
                label={t('dashboard.pill.in')}
                value={series ? formatMoney(series.total_income_minor, currency) : '—'}
              />
              <MoneyPill
                tone="out"
                label={t('dashboard.pill.out')}
                value={series ? formatMoney(series.total_expenses_minor, currency) : '—'}
              />
            </div>
          </div>
          <CashFlowPulse
            series={series}
            formatAmount={(minor) => formatMoney(minor, currency)}
            // Grows with the drop zone beside it, so the pane has no empty band.
            className="mx-5 mt-3 mb-5 min-h-[72px] flex-1"
            label={
              series && range
                ? t('dashboard.light.label', {
                    income: formatMoney(series.total_income_minor, currency),
                    expenses: formatMoney(series.total_expenses_minor, currency),
                    net: formatMoney(series.net_minor, currency, undefined, { signed: true }),
                    range,
                  })
                : t('dashboard.light.empty')
            }
          />
        </section>

        <section className="glass-pane rounded-[20px] p-2">
          <DocumentDropZone
            entityId={entity.id}
            onSuggestion={(s, source) => {
              setError(null)
              applySuggestion(s, source)
              if (s.source === 'none' && !s.amount_minor) {
                setError(renderUiTexts(s.notes, currency) || t('tx.couldNotReadDoc'))
              }
            }}
            onError={(msg) => setError(msg)}
          />
        </section>
      </div>

      <div className="glass-pane grid gap-2 rounded-[20px] p-2 md:grid-cols-2 lg:grid-cols-[minmax(0,1fr)_9.5rem_9.5rem_12rem]">
        <Field label={t('tx.search')}>
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t('tx.searchPlaceholder')}
          />
        </Field>
        <Field label={t('tx.from')}>
          <DateInput value={fromDate} onChange={setFromDate} aria-label={t('tx.filterFrom')} />
        </Field>
        <Field label={t('tx.to')}>
          <DateInput value={toDate} onChange={setToDate} aria-label={t('tx.filterTo')} />
        </Field>
        <Field label={t('tx.account')}>
          <Select
            value={accountFilter}
            onChange={(e) => setAccountFilter(e.target.value)}
            aria-label={t('tx.account')}
          >
            <option value="">{t('tx.allAccounts')}</option>
            {accounts
              .filter((a) => a.is_active)
              .map((a) => (
                <option key={a.id} value={a.id}>
                  {a.code} · {a.name}
                </option>
              ))}
          </Select>
        </Field>
      </div>

      <Modal
        open={showForm}
        title={editId ? t('tx.editTitle') : t('tx.newTitle')}
        description={editId ? t('tx.editDescription') : t('tx.newDescription')}
        onClose={closeForm}
      >
        <div className="mb-5">
          <Segmented<EntryKind>
            value={kind}
            onChange={setKindAndDefaults}
            options={[
              {
                id: 'expense',
                label: t('kind.expense'),
                icon: <ArrowUpRight className="size-3.5" />,
                tone: 'money-out',
              },
              {
                id: 'income',
                label: t('kind.income'),
                icon: <ArrowDownLeft className="size-3.5" />,
                tone: 'money-in',
              },
              {
                id: 'bill',
                label: t('kind.bill'),
                icon: <FileText className="size-3.5" />,
                tone: 'money-out',
              },
              {
                id: 'transfer',
                label: t('kind.transfer'),
                icon: <ArrowLeftRight className="size-3.5" />,
              },
            ]}
          />
        </div>

        {scanNotes && scanNotes.length > 0 ? (
          <div className="mb-5 rounded-xl border border-[var(--color-accent)]/25 bg-[var(--color-accent-soft)] px-4 py-3 text-xs text-[var(--color-fg-secondary)]">
            {renderUiTexts(scanNotes, currency)}
            {pendingDoc ? (
              <span className="mt-1 block text-[var(--color-muted)]">
                {t('tx.docWillStore')}
                {amount
                  ? t('tx.docSuggested', {
                      amount: fmtMoney(parseMajorToMinor(amount, currency) ?? 0, currency),
                    })
                  : ''}
                {t('tx.docReview')}
              </span>
            ) : null}
          </div>
        ) : null}

        <form onSubmit={onPost} className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <Field label={t('tx.date')}>
            <DateInput value={date} onChange={setDate} required aria-label={t('tx.entryDate')} />
          </Field>
          <Field label={t('tx.amount', { ccy })}>
            <Input
              inputMode="decimal"
              placeholder={t('tx.amountPlaceholder')}
              value={amount}
              onChange={(e) => {
                setAmount(e.target.value)
                setAmountInvalid(false)
              }}
              className="tabular-nums"
              required
              aria-invalid={amountInvalid || undefined}
              aria-describedby={amountInvalid ? errorBannerId : undefined}
            />
          </Field>
          <Field label={t('tx.reference')}>
            <Input
              value={reference}
              onChange={(e) => setReference(e.target.value)}
              placeholder={t('tx.referencePlaceholder')}
            />
          </Field>

          <Field label={t('tx.descriptionLabel')} className="sm:col-span-2 lg:col-span-3">
            <Input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={
                kind === 'bill'
                  ? t('tx.descPlaceholder.bill')
                  : kind === 'income'
                    ? t('tx.descPlaceholder.income')
                    : kind === 'transfer'
                      ? t('tx.descPlaceholder.transfer')
                      : t('tx.descPlaceholder.expense')
              }
              required
            />
          </Field>

          {kind === 'expense' ? (
            <>
              <Field label={t('tx.categoryWhatFor')}>
                <Select value={categoryId} onChange={(e) => setCategoryId(e.target.value)} required>
                  {expenseAccounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.code} · {a.name}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label={t('tx.paidFrom')}>
                <Select value={walletId} onChange={(e) => setWalletId(e.target.value)} required>
                  {walletAccounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.code} · {a.name}
                    </option>
                  ))}
                </Select>
              </Field>
            </>
          ) : null}

          {kind === 'income' ? (
            <>
              <Field label={t('tx.incomeType')}>
                <Select value={categoryId} onChange={(e) => setCategoryId(e.target.value)} required>
                  {incomeAccounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.code} · {a.name}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label={t('tx.receivedInto')}>
                <Select value={walletId} onChange={(e) => setWalletId(e.target.value)} required>
                  {walletAccounts
                    .filter((a) => a.account_type === 'asset')
                    .map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.code} · {a.name}
                      </option>
                    ))}
                </Select>
              </Field>
            </>
          ) : null}

          {kind === 'bill' ? (
            <>
              <Field label={t('tx.billStatus')} className="sm:col-span-2 lg:col-span-3">
                <Select
                  value={billStatus}
                  onChange={(e) => setBillStatus(e.target.value as BillStatus)}
                >
                  <option value="paid">{t('tx.billPaidNow')}</option>
                  <option value="unpaid">{t('tx.billUnpaid')}</option>
                  <option value="pay_existing">{t('tx.billPayExisting')}</option>
                </Select>
              </Field>
              {billStatus !== 'pay_existing' ? (
                <Field label={t('tx.billCategory')}>
                  <Select
                    value={categoryId}
                    onChange={(e) => setCategoryId(e.target.value)}
                    required
                  >
                    {expenseAccounts.map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.code} · {a.name}
                      </option>
                    ))}
                  </Select>
                </Field>
              ) : null}
              {billStatus === 'paid' || billStatus === 'pay_existing' ? (
                <Field label={billStatus === 'paid' ? t('tx.paidFrom') : t('tx.payFrom')}>
                  <Select value={walletId} onChange={(e) => setWalletId(e.target.value)} required>
                    {walletAccounts.map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.code} · {a.name}
                      </option>
                    ))}
                  </Select>
                </Field>
              ) : null}
              {billStatus === 'unpaid' || billStatus === 'pay_existing' ? (
                <Field label={t('tx.billsPayableAccount')}>
                  <Select value={payableId} onChange={(e) => setPayableId(e.target.value)} required>
                    {payableAccounts.length === 0 ? (
                      <option value="">{t('tx.noLiability')}</option>
                    ) : null}
                    {payableAccounts.map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.code} · {a.name}
                      </option>
                    ))}
                  </Select>
                </Field>
              ) : null}
              {payableAccounts.length === 0 && billStatus !== 'paid' ? (
                <p className="text-xs text-[var(--color-muted)] sm:col-span-2 lg:col-span-3">
                  {t('tx.billsPayableTip')}
                </p>
              ) : null}
            </>
          ) : null}

          {kind === 'transfer' ? (
            <>
              <Field label={t('tx.from')}>
                <Select value={fromId} onChange={(e) => setFromId(e.target.value)} required>
                  {walletAccounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.code} · {a.name}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label={t('tx.to')}>
                <Select value={toId} onChange={(e) => setToId(e.target.value)} required>
                  {walletAccounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.code} · {a.name}
                    </option>
                  ))}
                </Select>
              </Field>
            </>
          ) : null}

          <div className="flex justify-end gap-2 border-t border-[var(--color-border)] pt-4 sm:col-span-2 lg:col-span-3">
            <Button
              type="button"
              variant="secondary"
              disabled={busy}
              onClick={closeForm}
            >
              {t('common.cancel')}
            </Button>
            <Button type="submit" busy={busy}>
              {busy ? t('common.saving') : editId ? t('tx.saveChanges') : t('tx.saveEntry')}
            </Button>
          </div>
        </form>
      </Modal>

      <EntryDetailModal
        view={detailView}
        accounts={accountMap}
        documents={detailId ? (docsByEntry.get(detailId) ?? []) : []}
        currency={currency}
        // DocumentViewerModal stacks above this one. The dialog stack routes
        // Escape to the top-most dialog only; this guard is defense in depth
        // so the detail modal can never close while the viewer sits above it.
        onClose={() => {
          if (!viewerDocId) setDetailId(null)
        }}
        onEdit={() => {
          if (detailView) startEdit(detailView)
        }}
        onView={(id) => setViewerDocId(id)}
        onChanged={reload}
        onError={(msg) => setError(msg)}
      />

      <DocumentViewerModal
        documentId={viewerDocId}
        onClose={() => setViewerDocId(null)}
        onError={(msg) => setError(msg)}
      />

      {visibleEntries.length === 0 && filtersActive ? (
        <EmptyState
          icon={<FileText className="size-5" />}
          title={t('tx.noMatchTitle')}
          body={t('tx.noMatchBody')}
        />
      ) : visibleEntries.length === 0 ? (
        <EmptyState
          icon={<ArrowLeftRight className="size-5" />}
          title={t('tx.emptyTitle')}
          body={t('tx.emptyBody')}
          action={<div className="flex flex-wrap items-center justify-center gap-2">{csvActions}</div>}
        />
      ) : (
        <Panel
          title={t('tx.allEntries')}
          description={`${t('tx.list.meta.counts', { posted: postedCount, hidden: hiddenCount })} · ${ccy}`}
          whisper={t('tx.export.whisper')}
          actions={csvActions}
        >
          <ul className="divide-y divide-[var(--color-border)]">
            {visibleEntries.map((view) => {
              const amountMinor = view.lines.reduce((s, l) => s + l.debit.amount_minor, 0)
              const kindLabel = inferKind(view, accountMap)
              const parts = view.lines
                .map((l) => {
                  const acc = accountMap.get(l.account_id)
                  const side = l.debit.amount_minor > 0 ? '→' : '←'
                  return `${side} ${acc?.name ?? '?'}`
                })
                .join('  ')
              const signed =
                kindLabel === 'expense'
                  ? -amountMinor
                  : kindLabel === 'income'
                    ? amountMinor
                    : amountMinor
              const tone =
                kindLabel === 'income' ? 'money-in' : kindLabel === 'expense' ? 'money-out' : 'muted'

              return (
                <li
                  key={view.entry.id}
                  onClick={() => setDetailId(view.entry.id)}
                  className="group flex cursor-pointer items-center gap-4 px-5 py-3 transition hover:bg-white/[0.05] focus-within:bg-white/[0.05]"
                >
                  <IconBadge tone={tone}>
                    {kindLabel === 'income' ? (
                      <ArrowDownLeft className="size-4" />
                    ) : kindLabel === 'expense' ? (
                      <ArrowUpRight className="size-4" />
                    ) : (
                      <ArrowLeftRight className="size-4" />
                    )}
                  </IconBadge>
                  {/* Keyboard path: activating this button bubbles its click
                      to the row handler above — no duplicate handler. */}
                  <button type="button" className="min-w-0 flex-1 text-left">
                    <div className="flex min-w-0 items-center gap-1.5">
                      <span className="truncate text-sm font-medium text-[var(--color-fg)]">
                        {view.entry.description}
                      </span>
                      {view.entry.hidden ? <HiddenBadge /> : null}
                    </div>
                    <div className="truncate text-xs text-[var(--color-muted)] tabular-nums">
                      {formatDate(view.entry.entry_date)}
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      <span>{t(`kind.${kindLabel}`)}</span>
                      {view.entry.reference ? (
                        <span>{t('entry.ref', { reference: view.entry.reference })}</span>
                      ) : null}
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      {parts}
                    </div>
                  </button>
                  {/* Fixed slots: the paperclip keeps one column whatever the
                      amount's width, and amounts right-align in their own. */}
                  <span className="flex size-4 shrink-0 items-center justify-center">
                    {(docsByEntry.get(view.entry.id)?.length ?? 0) > 0 ? (
                      <Paperclip className="size-4 text-[var(--color-muted)]" aria-label={t('tx.hasDocument')} />
                    ) : null}
                  </span>
                  {/* Amount and its delete slot sit as one group, 8px apart. */}
                  <span className="flex shrink-0 items-center gap-2">
                    <span className="flex min-w-32 justify-end">
                      <AmountPill tone={kindLabel === 'income' ? 'in' : kindLabel === 'expense' ? 'out' : 'neutral'}>
                        {formatMoney(signed, currency, undefined, {
                          signed: kindLabel === 'expense' || kindLabel === 'income',
                        })}
                      </AmountPill>
                    </span>
                    <Button
                      variant="ghost"
                      size="iconSm"
                      className="opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100"
                      onClick={(e) => {
                        e.stopPropagation()
                        setVoidId(view.entry.id)
                      }}
                      aria-label={t('tx.deleteEntry')}
                      title={t('common.delete')}
                    >
                      <Trash2 className="size-4" />
                    </Button>
                  </span>
                </li>
              )
            })}
          </ul>
        </Panel>
      )}
    </div>
  )
}
