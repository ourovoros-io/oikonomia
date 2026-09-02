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
  type CsvImportPreview,
  type CsvColumnMapping,
  type DocumentMeta,
  type Entity,
  type LastRoleAccounts,
  type PendingDocSource,
  type PostedEntryView,
  type SimpleEntryInput,
} from '../lib/api'
import { currencyFractionDigits, parseMajorToMinor } from '../lib/money'
import { fileToBase64, mimeFromName } from '../lib/files'
import { beginExclusive } from '../lib/guards'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { CsvMappingModal } from '../components/CsvMappingModal'
import { CsvPreviewModal } from '../components/CsvPreviewModal'
import { DateInput } from '../components/DateInput'
import { DocumentDropZone } from '../components/DocumentDropZone'
import { DocumentViewerModal } from '../components/DocumentViewerModal'
import { EntryDetailModal } from '../components/EntryDetailModal'
import { HiddenBadge } from '../components/hiddenUi'
import { Modal } from '../components/Modal'
import { csvImportAccountDefaults, mappingsEqual } from '../lib/csvImport'
import { lastAccountsMapKey } from '../lib/simpleEntry'
import {
  Button,
  EmptyState,
  ErrorBanner,
  Field,
  IconBadge,
  Input,
  PageHeader,
  Panel,
  Segmented,
  Select,
} from '../components/ui'
import { cn } from '../lib/cn'
import { commandErrorMessage } from '../lib/commandError'
import type { CommandError } from '../lib/tauri'
import type { DocumentSuggestion } from '../lib/api'
import { formatMoney as fmtMoney } from '../lib/money'
import { useI18n } from '../lib/I18nProvider'
import { RecurringPage } from './RecurringPage'

type Props = { entity: Entity | null; onCreateBook?: () => void }

/** High-level entry kinds so users don't think in debit/credit. */
type EntryKind = 'expense' | 'income' | 'bill' | 'transfer'

type BillStatus = 'paid' | 'unpaid' | 'pay_existing'

function pickDefault(
  accounts: Account[],
  type: Account['account_type'],
  nameHints: string[] = [],
): string {
  const active = accounts.filter((a) => a.is_active && a.account_type === type)
  for (const hint of nameHints) {
    const found = active.find((a) => a.name.toLowerCase().includes(hint.toLowerCase()))
    if (found) return found.id
  }
  return active[0]?.id ?? ''
}

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

export function TransactionsPage({ entity, onCreateBook }: Props) {
  const { t } = useI18n()
  const [entries, setEntries] = useState<PostedEntryView[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])
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
  const [scanNotes, setScanNotes] = useState<string | null>(null)
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
  const prevEntityId = useRef<string | null>(null)
  const busyRef = useRef(false)
  const csvBusyRef = useRef(false)

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

  const expenseAccounts = useMemo(() => accountsOf(accounts, ['expense']), [accounts])
  const incomeAccounts = useMemo(() => accountsOf(accounts, ['income']), [accounts])
  const walletAccounts = useMemo(
    () =>
      accountsOf(accounts, ['asset', 'liability']).filter((a) => {
        // Prefer money accounts; exclude pure equity-like names
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
  const payableAccounts = useMemo(() => {
    const liab = accountsOf(accounts, ['liability'])
    const preferred = liab.filter((a) => {
      const n = a.name.toLowerCase()
      return n.includes('payable') || n.includes('bill') || n.includes('ap')
    })
    return preferred.length > 0 ? preferred : liab
  }, [accounts])

  function applyKindDefaults(nextKind: EntryKind, list: Account[]) {
    if (nextKind === 'expense') {
      setCategoryId(pickDefault(list, 'expense', ['food', 'utilities', 'bills', 'other']))
      setWalletId(pickDefault(list, 'asset', ['checking', 'bank', 'cash']))
    } else if (nextKind === 'income') {
      setCategoryId(pickDefault(list, 'income', ['salary', 'sales', 'freelance']))
      setWalletId(pickDefault(list, 'asset', ['checking', 'bank', 'cash']))
    } else if (nextKind === 'bill') {
      setCategoryId(
        pickDefault(list, 'expense', ['utilities', 'bills', 'housing', 'subscription', 'rent']),
      )
      setWalletId(pickDefault(list, 'asset', ['checking', 'bank', 'cash']))
      setPayableId(pickDefault(list, 'liability', ['bills payable', 'accounts payable', 'payable']))
      setBillStatus('paid')
    } else {
      setFromId(pickDefault(list, 'asset', ['checking', 'bank']))
      const savings = pickDefault(list, 'asset', ['savings', 'cash'])
      setToId(savings || pickDefault(list, 'asset', []))
    }
  }

  async function reload() {
    if (!entity) return
    const [e, a, d] = await Promise.all([
      api.entryList(entity.id, {
        search: debouncedSearch.trim() || undefined,
        from: fromDate || undefined,
        to: toDate || undefined,
        accountId: accountFilter || undefined,
      }),
      api.accountList(entity.id),
      api.documentList(entity.id),
    ])
    setEntries(e)
    setAccounts(a)
    setDocs(d)
    if (!categoryId && !walletId) {
      applyKindDefaults(kind, a)
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
      const defaults = csvImportAccountDefaults({
        accounts,
        walletId,
        categoryId,
        kind,
        expenseLast: last.expenseLast,
        incomeLast: last.incomeLast,
      })
      const preview = await api.csvImportPreview({
        entity_id: entity.id,
        wallet_account_id: defaults.wallet_account_id,
        expense_account_id: defaults.expense_account_id,
        income_account_id: defaults.income_account_id,
      })
      if (!preview) return
      setCsvRoles(defaults)
      setCsvPreview(preview)
      setCsvStep('mapping')
    } catch (err) {
      setError(commandErrorMessage(err as CommandError))
    } finally {
      csvBusyRef.current = false
      setCsvBusy(null)
    }
  }

  async function onMappingContinue(mapping: CsvColumnMapping, unchanged: boolean) {
    if (!entity || !csvPreview) return
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
      setError(commandErrorMessage(err as CommandError))
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
      setError(commandErrorMessage(err as CommandError))
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
      setError(commandErrorMessage(err as CommandError))
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
    applyKindDefaults('expense', accounts)
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
    void reload().catch((err) => setError(commandErrorMessage(err as CommandError)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity?.id, debouncedSearch, fromDate, toDate, accountFilter])

  function setKindAndDefaults(next: EntryKind) {
    setKind(next)
    applyKindDefaults(next, accounts)
  }

  function applySuggestion(s: DocumentSuggestion, source: PendingDocSource) {
    resetDraft()
    setShowForm(true)
    setPendingDoc(source)
    setPendingAnalysis(JSON.stringify(s))
    setScanNotes(s.notes)

    if (s.kind === 'bill') {
      setKind('bill')
      applyKindDefaults('bill', accounts)
      setBillStatus(s.bill_unpaid ? 'unpaid' : 'paid')
    } else if (s.kind === 'income') {
      setKind('income')
      applyKindDefaults('income', accounts)
    } else {
      setKind('expense')
      applyKindDefaults('expense', accounts)
    }

    if (s.entry_date) setDate(s.entry_date)
    if (s.description) setDescription(s.description)
    else if (s.merchant) setDescription(s.merchant)
    if (s.reference) setReference(s.reference)
    // The analyzer emits 2-exponent minor units; the backend already clears
    // amounts for other currencies — this guard is defense in depth.
    const digits = currencyFractionDigits(entity?.base_currency ?? 'EUR')
    if (s.amount_minor != null && s.amount_minor > 0 && digits === 2) {
      setAmount((s.amount_minor / 100).toFixed(2))
    }
    if (s.category_account_id) setCategoryId(s.category_account_id)
    if (s.wallet_account_id) setWalletId(s.wallet_account_id)
    if (s.payable_account_id) setPayableId(s.payable_account_id)
  }

  async function onPost(ev: FormEvent) {
    ev.preventDefault()
    if (!entity) return
    if (!beginExclusive(busyRef)) return
    const minor = parseMajorToMinor(amount, entity.base_currency)
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
      setError(commandErrorMessage(err as CommandError))
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
      setError(commandErrorMessage(err as CommandError) || t('tx.deleteFailed'))
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

  if (subview === 'recurring') {
    return <RecurringPage entity={entity} onBack={() => setSubview('journal')} />
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
      <Button size="sm" onClick={openNewEntry}>
        <Plus className="size-3" />
        {t('tx.newEntry')}
      </Button>
    </>
  )

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

    const digits = currencyFractionDigits(ccy)
    const amountMinor = view.lines.reduce((s, l) => s + l.debit.amount_minor, 0)
    setAmount((amountMinor / 10 ** digits).toFixed(digits))
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
    <div className="space-y-6">
      <PageHeader
        eyebrow={t('tx.eyebrow')}
        title={t('tx.title')}
        description={t('tx.description')}
        meta={t('tx.meta')}
      />

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
        currency={ccy}
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

      <DocumentDropZone
        entityId={entity.id}
        onSuggestion={(s, source) => {
          setError(null)
          applySuggestion(s, source)
          if (s.source === 'none' && !s.amount_minor) {
            setError(s.notes || t('tx.couldNotReadDoc'))
          }
        }}
        onError={(msg) => setError(msg)}
      />

      <div className="flex flex-wrap items-end gap-3">
        <Field label={t('tx.search')} className="min-w-[220px] flex-1">
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t('tx.searchPlaceholder')}
          />
        </Field>
        <Field label={t('tx.from')} className="w-44">
          <DateInput value={fromDate} onChange={setFromDate} aria-label={t('tx.filterFrom')} />
        </Field>
        <Field label={t('tx.to')} className="w-44">
          <DateInput value={toDate} onChange={setToDate} aria-label={t('tx.filterTo')} />
        </Field>
        <Field label={t('tx.account')} className="w-56">
          <Select value={accountFilter} onChange={(e) => setAccountFilter(e.target.value)}>
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
              },
              {
                id: 'income',
                label: t('kind.income'),
                icon: <ArrowDownLeft className="size-3.5" />,
              },
              {
                id: 'bill',
                label: t('kind.bill'),
                icon: <FileText className="size-3.5" />,
              },
              {
                id: 'transfer',
                label: t('kind.transfer'),
                icon: <ArrowLeftRight className="size-3.5" />,
              },
            ]}
          />
        </div>

        {scanNotes ? (
          <div className="mb-5 rounded-xl border border-[var(--color-accent)]/25 bg-[var(--color-accent-soft)] px-4 py-3 text-xs text-[var(--color-fg-secondary)]">
            {scanNotes}
            {pendingDoc ? (
              <span className="mt-1 block text-[var(--color-muted)]">
                {t('tx.docWillStore')}
                {amount
                  ? t('tx.docSuggested', {
                      amount: fmtMoney(parseMajorToMinor(amount, ccy) ?? 0, ccy),
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
        currency={ccy}
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
                kindLabel === 'income' ? 'success' : kindLabel === 'expense' ? 'danger' : 'muted'

              return (
                <li
                  key={view.entry.id}
                  onClick={() => setDetailId(view.entry.id)}
                  className="flex cursor-pointer items-center gap-4 px-5 py-3.5 transition hover:bg-[var(--color-surface-2)]/50"
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
                    <div className="truncate text-xs text-[var(--color-muted)]">
                      {formatDate(view.entry.entry_date)}
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      <span>{t(`kind.${kindLabel}`)}</span>
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      {parts}
                    </div>
                  </button>
                  {(docsByEntry.get(view.entry.id)?.length ?? 0) > 0 ? (
                    <Paperclip
                      className="size-3.5 shrink-0 text-[var(--color-muted)]"
                      aria-label={t('tx.hasDocument')}
                    />
                  ) : null}
                  <div
                    className={cn(
                      'shrink-0 text-sm font-semibold tabular-nums',
                      kindLabel === 'expense'
                        ? 'text-[var(--color-danger)]'
                        : kindLabel === 'income'
                          ? 'text-[var(--color-success)]'
                          : 'text-[var(--color-fg)]',
                    )}
                  >
                    {formatMoney(signed, ccy, undefined, {
                      signed: kindLabel === 'expense' || kindLabel === 'income',
                    })}
                  </div>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    onClick={(e) => {
                      e.stopPropagation()
                      setVoidId(view.entry.id)
                    }}
                    aria-label={t('tx.deleteEntry')}
                    title={t('common.delete')}
                  >
                    <Trash2 className="size-4" />
                  </Button>
                </li>
              )
            })}
          </ul>
        </Panel>
      )}
    </div>
  )
}
