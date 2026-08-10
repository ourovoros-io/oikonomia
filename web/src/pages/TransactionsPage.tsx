import { useEffect, useMemo, useState, type FormEvent } from 'react'
import { ArrowDownLeft, ArrowLeftRight, ArrowUpRight, FileText, Plus, Trash2 } from 'lucide-react'
import {
  api,
  formatDate,
  formatMoney,
  todayISO,
  type Account,
  type Entity,
  type PostedEntryView,
} from '../lib/api'
import { currencyFractionDigits, parseMajorToMinor } from '../lib/money'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DocumentDropZone } from '../components/DocumentDropZone'
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
  cn,
} from '../components/ui'
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
  const [linkedDocumentId, setLinkedDocumentId] = useState<string | null>(null)
  const [scanNotes, setScanNotes] = useState<string | null>(null)

  const accountMap = useMemo(() => new Map(accounts.map((a) => [a.id, a])), [accounts])

  /** Hide voided pairs; the backend marks both sides via is_voided. */
  const visibleEntries = useMemo(() => entries.filter((e) => !e.is_voided), [entries])

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
    const [e, a] = await Promise.all([api.entryList(entity.id), api.accountList(entity.id)])
    setEntries(e)
    setAccounts(a)
    if (!categoryId && !walletId) {
      applyKindDefaults(kind, a)
    }
  }

  useEffect(() => {
    if (!entity) {
      setEntries([])
      setAccounts([])
      return
    }
    void reload().catch((err) => setError((err as CommandError).message))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity?.id])

  function setKindAndDefaults(next: EntryKind) {
    setKind(next)
    applyKindDefaults(next, accounts)
  }

  function applySuggestion(s: DocumentSuggestion) {
    setShowForm(true)
    setLinkedDocumentId(s.document_id)
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
      const posted = await api.entryPostSimple({
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
      })
      if (linkedDocumentId) {
        try {
          await api.documentLinkEntry(linkedDocumentId, posted.entry.id)
        } catch {
          /* entry is saved; link is best-effort */
        }
      }
      setDescription('')
      setReference('')
      setAmount('')
      setLinkedDocumentId(null)
      setScanNotes(null)
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
        onSuggestion={(s) => {
          setError(null)
          applySuggestion(s)
          if (s.source === 'none' && !s.amount_minor) {
            setError(s.notes || 'Could not read the document — fill the form manually.')
          }
        }}
        onError={(msg) => setError(msg)}
      />

      <Modal
        open={showForm}
        title="New entry"
        description="Pick a type — no debit/credit bookkeeping required"
        onClose={() => {
          if (!busy) setShowForm(false)
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
            {linkedDocumentId ? (
              <span className="mt-1 block text-[var(--color-muted)]">
                Document stored encrypted in your vault
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
              onClick={() => setShowForm(false)}
            >
              Cancel
            </Button>
            <Button type="submit" busy={busy}>
              {busy ? 'Saving…' : 'Save entry'}
            </Button>
          </div>
        </form>
      </Modal>

      {visibleEntries.length === 0 ? (
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
              <Plus className="size-4" />
              New entry
            </Button>
          }
        />
      ) : (
        <Panel
          title="All entries"
          description={`${visibleEntries.length} posted · ${ccy}`}
          icon={<FileText className="size-4" />}
          actions={
            <Button
              size="sm"
              onClick={() => {
                applyKindDefaults(kind, accounts)
                setShowForm(true)
              }}
            >
              <Plus className="size-3.5" />
              New entry
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
                  className="flex items-center gap-4 px-5 py-3.5 transition hover:bg-[var(--color-surface-2)]/50"
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
                  <div className="min-w-0 flex-1">
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
                  </div>
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
                    onClick={() => setVoidId(view.entry.id)}
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
