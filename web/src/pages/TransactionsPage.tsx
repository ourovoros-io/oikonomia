import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react'
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  FileText,
  Paperclip,
  Plus,
  Trash2,
} from 'lucide-react'
import {
  api,
  formatDate,
  formatMoney,
  isoDate,
  todayISO,
  type Account,
  type DocumentMeta,
  type Entity,
  type PendingDocSource,
  type PostedEntryView,
} from '../lib/api'
import { currencyFractionDigits, parseMajorToMinor } from '../lib/money'
import { fileToBase64, mimeFromName } from '../lib/files'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DocumentDropZone } from '../components/DocumentDropZone'
import { DocumentViewerModal } from '../components/DocumentViewerModal'
import { EntryDetailModal } from '../components/EntryDetailModal'
import { Modal } from '../components/Modal'
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
import type { CommandError } from '../lib/tauri'
import type { DocumentSuggestion } from '../lib/api'
import { formatMoney as fmtMoney } from '../lib/money'

type Props = { entity: Entity | null }

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

export function TransactionsPage({ entity }: Props) {
  const [entries, setEntries] = useState<PostedEntryView[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])
  const [error, setError] = useState<string | null>(null)
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
  const prevEntityId = useRef<string | null>(null)

  const accountMap = useMemo(() => new Map(accounts.map((a) => [a.id, a])), [accounts])

  /** Hide voided pairs; the backend marks both sides via is_voided. */
  const visibleEntries = useMemo(() => entries.filter((e) => !e.is_voided), [entries])

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
    void reload().catch((err) => setError((err as CommandError).message))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity?.id, debouncedSearch, fromDate, toDate, accountFilter])

  function setKindAndDefaults(next: EntryKind) {
    setKind(next)
    applyKindDefaults(next, accounts)
  }

  function applySuggestion(s: DocumentSuggestion, source: PendingDocSource) {
    setShowForm(true)
    setPendingDoc(source)
    setPendingAnalysis(JSON.stringify(s))
    setScanNotes(s.notes)

    if (s.kind === 'bill') {
      setKind('bill')
      setBillStatus(s.bill_unpaid ? 'unpaid' : 'paid')
    } else if (s.kind === 'income') {
      setKind('income')
    } else {
      setKind('expense')
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
    const minor = parseMajorToMinor(amount, entity.base_currency)
    if (minor === null || minor <= 0) {
      setError('Enter a valid amount (e.g. 25.50 or 25,50)')
      return
    }
    // Role/account rules live in Rust (post_simple_entry); its Validation
    // errors surface in the banner below.

    setBusy(true)
    setError(null)
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
      setDescription('')
      setReference('')
      setAmount('')
      setPendingDoc(null)
      setPendingAnalysis(null)
      setScanNotes(null)
      setEditId(null)
      setShowForm(false)
      await reload()
    } catch (err) {
      setError((err as CommandError).message)
    } finally {
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
      setError((err as CommandError).message || 'Failed to delete entry')
    } finally {
      setVoidBusy(false)
    }
  }

  if (!entity) {
    return (
      <EmptyState
        icon={<ArrowLeftRight className="size-5" />}
        title="No book selected"
        body="Create or select a book first."
      />
    )
  }

  const ccy = entity.base_currency

  /**
   * Map a posted entry's lines back onto the simple form and open it for
   * editing. Bills reopen as their expense/transfer equivalent — the journal
   * lines are identical, so nothing is lost.
   */
  function startEdit(view: PostedEntryView) {
    const debit = view.lines.find((l) => l.debit.amount_minor > 0)
    const credit = view.lines.find((l) => l.credit.amount_minor > 0)
    if (!debit || !credit) return

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
        eyebrow="Ledger"
        title="Transactions"
        description="Record money in, money out, bills, and transfers."
        meta="Offline OCR · encrypted docs"
      />

      <ErrorBanner message={error} />

      <ConfirmDialog
        open={voidId !== null}
        title="Delete entry?"
        body="The entry will be removed from your list. A reversing journal entry is kept in the books for audit (you will not see it here)."
        confirmLabel="Delete"
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
            setError(s.notes || 'Could not read the document — fill the form manually.')
          }
        }}
        onError={(msg) => setError(msg)}
      />

      <div className="flex flex-wrap items-end gap-3">
        <Field label="Search" className="min-w-[220px] flex-1">
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Description, reference, memo…"
          />
        </Field>
        <Field label="From" className="w-40">
          <Input type="date" value={fromDate} onChange={(e) => setFromDate(e.target.value)} />
        </Field>
        <Field label="To" className="w-40">
          <Input type="date" value={toDate} onChange={(e) => setToDate(e.target.value)} />
        </Field>
        <Field label="Account" className="w-56">
          <Select value={accountFilter} onChange={(e) => setAccountFilter(e.target.value)}>
            <option value="">All accounts</option>
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
        title={editId ? 'Edit entry' : 'New entry'}
        description={
          editId
            ? 'Replaces the original entry — the books keep an audit trail'
            : 'Pick a type — no debit/credit bookkeeping required'
        }
        onClose={() => {
          if (!busy) {
            setShowForm(false)
            setPendingDoc(null)
            setPendingAnalysis(null)
            setEditId(null)
          }
        }}
      >
        <div className="mb-5">
          <Segmented<EntryKind>
            value={kind}
            onChange={setKindAndDefaults}
            options={[
              {
                id: 'expense',
                label: 'Expense',
                icon: <ArrowUpRight className="size-3.5" />,
              },
              {
                id: 'income',
                label: 'Income',
                icon: <ArrowDownLeft className="size-3.5" />,
              },
              {
                id: 'bill',
                label: 'Bill',
                icon: <FileText className="size-3.5" />,
              },
              {
                id: 'transfer',
                label: 'Transfer',
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
                Document will be stored encrypted when you save
                {amount ? ` · suggested ${fmtMoney(parseMajorToMinor(amount, ccy) ?? 0, ccy)}` : ''}
                . Review fields, then save.
              </span>
            ) : null}
          </div>
        ) : null}

        <form onSubmit={onPost} className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <Field label="Date">
            <Input type="date" value={date} onChange={(e) => setDate(e.target.value)} required />
          </Field>
          <Field label={`Amount (${ccy})`}>
            <Input
              inputMode="decimal"
              placeholder="25,50 or 25.50"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              className="tabular-nums"
              required
            />
          </Field>
          <Field label="Reference (optional)">
            <Input
              value={reference}
              onChange={(e) => setReference(e.target.value)}
              placeholder="Invoice #, bill #…"
            />
          </Field>

          <Field label="Description" className="sm:col-span-2 lg:col-span-3">
            <Input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={
                kind === 'bill'
                  ? 'e.g. Electricity March'
                  : kind === 'income'
                    ? 'e.g. March salary'
                    : kind === 'transfer'
                      ? 'e.g. Move to savings'
                      : 'e.g. Groceries'
              }
              required
            />
          </Field>

          {kind === 'expense' ? (
            <>
              <Field label="Category (what for)">
                <Select value={categoryId} onChange={(e) => setCategoryId(e.target.value)} required>
                  {expenseAccounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.code} · {a.name}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label="Paid from">
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
              <Field label="Income type">
                <Select value={categoryId} onChange={(e) => setCategoryId(e.target.value)} required>
                  {incomeAccounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.code} · {a.name}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label="Received into">
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
              <Field label="Bill status" className="sm:col-span-2 lg:col-span-3">
                <Select
                  value={billStatus}
                  onChange={(e) => setBillStatus(e.target.value as BillStatus)}
                >
                  <option value="paid">Paid now (from bank/card)</option>
                  <option value="unpaid">Unpaid — I owe this (bills payable)</option>
                  <option value="pay_existing">Pay an existing unpaid bill</option>
                </Select>
              </Field>
              {billStatus !== 'pay_existing' ? (
                <Field label="Bill category">
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
                <Field label={billStatus === 'paid' ? 'Paid from' : 'Pay from'}>
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
                <Field label="Bills payable account">
                  <Select value={payableId} onChange={(e) => setPayableId(e.target.value)} required>
                    {payableAccounts.length === 0 ? (
                      <option value="">No liability accounts — add one</option>
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
                  Tip: under Accounts, add a liability named “Bills Payable”. New Personal books
                  include this by default.
                </p>
              ) : null}
            </>
          ) : null}

          {kind === 'transfer' ? (
            <>
              <Field label="From">
                <Select value={fromId} onChange={(e) => setFromId(e.target.value)} required>
                  {walletAccounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.code} · {a.name}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label="To">
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
              onClick={() => {
                setShowForm(false)
                setPendingDoc(null)
                setPendingAnalysis(null)
                setEditId(null)
              }}
            >
              Cancel
            </Button>
            <Button type="submit" busy={busy}>
              {busy ? 'Saving…' : editId ? 'Save changes' : 'Save entry'}
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
          title="No matching entries"
          body="No entries match the current search or filters."
        />
      ) : visibleEntries.length === 0 ? (
        <EmptyState
          icon={<ArrowLeftRight className="size-5" />}
          title="No transactions yet"
          body="Use Expense for spending, Income for money in, and Bill for utilities or invoices you need to track."
          action={
            <Button
              onClick={() => {
                applyKindDefaults(kind, accounts)
                setShowForm(true)
              }}
            >
              <Plus className="size-3.5" />
              New Entry
            </Button>
          }
        />
      ) : (
        <Panel
          title="All entries"
          description={`${visibleEntries.length} posted · ${ccy}`}
          actions={
            <Button
              size="sm"
              onClick={() => {
                applyKindDefaults(kind, accounts)
                setShowForm(true)
              }}
            >
              <Plus className="size-3" />
              New Entry
            </Button>
          }
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
                    <div className="truncate text-sm font-medium text-[var(--color-fg)]">
                      {view.entry.description}
                    </div>
                    <div className="truncate text-xs text-[var(--color-muted)]">
                      {formatDate(view.entry.entry_date)}
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      <span className="capitalize">{kindLabel}</span>
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      {parts}
                    </div>
                  </button>
                  {(docsByEntry.get(view.entry.id)?.length ?? 0) > 0 ? (
                    <Paperclip
                      className="size-3.5 shrink-0 text-[var(--color-muted)]"
                      aria-label="Has attached document"
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
                    aria-label="Delete entry"
                    title="Delete"
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
