import { useCallback, useEffect, useMemo, useState, type FormEvent } from 'react'
import {
  api,
  todayISO,
  type Account,
  type Entity,
  type LastRoleAccounts,
  type UiPrefs,
} from '../lib/api'
import { parseMajorToMinor } from '../lib/money'
import type { CommandError } from '../lib/tauri'
import { Button, ErrorBanner, Input } from '../components/ui'
import { DateInput } from '../components/DateInput'
import { cn } from '../lib/cn'
import {
  accountsOf,
  buildSimpleEntryInput,
  kindDefaultAccounts,
  lastAccountsMapKey,
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

const KIND_OPTIONS: Array<{ id: EntryKind; label: string }> = [
  { id: 'expense', label: 'Expense' },
  { id: 'income', label: 'Income' },
  { id: 'bill', label: 'Bill' },
  { id: 'transfer', label: 'Transfer' },
]

const compactControl =
  'h-8 w-full min-w-0 rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] px-2 text-xs text-[var(--color-fg)] outline-none transition placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)] focus:ring-1 focus:ring-[var(--color-accent)]/25 disabled:opacity-50'

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

export function QuickAddPage({ onPosted, onBusyChange }: Props) {
  const [entities, setEntities] = useState<Entity[]>([])
  const [entityId, setEntityId] = useState<string | null>(null)
  const [accounts, setAccounts] = useState<Account[]>([])
  const [prefs, setPrefs] = useState<UiPrefs | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

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

  const entity = useMemo(
    () => entities.find((e) => e.id === entityId) ?? null,
    [entities, entityId],
  )

  const roleSetters = useMemo(
    () => ({ setCategoryId, setWalletId, setPayableId, setFromId, setToId }),
    [],
  )

  useEffect(() => {
    onBusyChange?.(busy)
  }, [busy, onBusyChange])

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

  // Initial load: entities + prefs, then accounts for resolved entity.
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

  // Focus amount once the form is ready.
  useEffect(() => {
    if (loading || !entityId) return
    const t = window.setTimeout(() => {
      document.getElementById('quick-add-amount')?.focus()
    }, 0)
    return () => window.clearTimeout(t)
  }, [loading, entityId])

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
    if (!entity || !prefs) {
      applyRoleState(kindDefaultAccounts(next, accounts), roleSetters)
      return
    }
    const key = lastAccountsMapKey(entity.id, next)
    const last = prefs.last_accounts_by_entity_kind[key]
    applyRoleState(resolveRoleAccounts(next, accounts, last), roleSetters)
  }

  async function onSubmit(ev: FormEvent) {
    ev.preventDefault()
    if (!entity || busy) return

    const minor = parseMajorToMinor(amount, entity.base_currency)
    if (minor === null || minor <= 0) {
      setError('Enter a valid amount (e.g. 25.50 or 25,50)')
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
      await api.entryPostSimple(input)

      const roles: LastRoleAccounts = {
        category_account_id: categoryId || null,
        wallet_account_id: walletId || null,
        payable_account_id: payableId || null,
        from_account_id: fromId || null,
        to_account_id: toId || null,
      }
      // Prefs are best-effort: a remember failure must not hide a successful post.
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

      // Clear busy before success UI so parent Escape/blur are not stuck blocked.
      setBusy(false)
      onBusyChange?.(false)
      onPosted({
        kind,
        amountMinor: minor,
        currency: entity.base_currency,
      })
    } catch (err) {
      setError((err as CommandError).message)
      setBusy(false)
    }
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
        <p className="text-[11px] text-[var(--color-muted)]">
          Quick add needs at least one book.
        </p>
        <Button size="sm" onClick={() => void api.openMainWindow()}>
          Open Oikonomia
        </Button>
      </div>
    )
  }

  if (!entity) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--color-muted)]">
        Loading…
      </div>
    )
  }

  const ccy = entity.base_currency

  return (
    <form
      onSubmit={(e) => void onSubmit(e)}
      className="flex h-full flex-col gap-1 p-1.5"
    >
      {/* Kind row */}
      <div className="inline-flex h-7 w-full items-stretch rounded-[var(--radius-control)] border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] p-0.5">
        {KIND_OPTIONS.map((opt) => {
          const active = kind === opt.id
          return (
            <button
              key={opt.id}
              type="button"
              aria-pressed={active}
              disabled={busy}
              onClick={() => onKindChange(opt.id)}
              className={cn(
                'min-w-0 flex-1 rounded-[calc(var(--radius-control)-2px)] px-1 text-[11px] font-medium transition',
                active
                  ? 'bg-[var(--color-surface)] text-[var(--color-fg)] shadow-sm'
                  : 'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
              )}
            >
              {opt.label}
            </button>
          )
        })}
      </div>

      {/* Amount + role accounts + date + Add */}
      <div className="flex min-w-0 items-center gap-1">
        <Input
          id="quick-add-amount"
          inputMode="decimal"
          placeholder={ccy}
          value={amount}
          onChange={(e) => setAmount(e.target.value)}
          className="h-8 w-[4.5rem] shrink-0 px-2 text-xs tabular-nums"
          aria-label={`Amount (${ccy})`}
          required
          disabled={busy}
        />

        {kind === 'expense' ? (
          <>
            <select
              className={cn(compactControl, 'flex-1')}
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
              required
              disabled={busy}
              aria-label="Category"
            >
              {expenseAccounts.length === 0 ? <option value="">No expense accounts</option> : null}
              {expenseAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            <select
              className={cn(compactControl, 'flex-1')}
              value={walletId}
              onChange={(e) => setWalletId(e.target.value)}
              required
              disabled={busy}
              aria-label="Paid from"
            >
              {walletAccounts.length === 0 ? <option value="">No wallet accounts</option> : null}
              {walletAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </>
        ) : null}

        {kind === 'income' ? (
          <>
            <select
              className={cn(compactControl, 'flex-1')}
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
              required
              disabled={busy}
              aria-label="Income type"
            >
              {incomeAccounts.length === 0 ? <option value="">No income accounts</option> : null}
              {incomeAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            <select
              className={cn(compactControl, 'flex-1')}
              value={walletId}
              onChange={(e) => setWalletId(e.target.value)}
              required
              disabled={busy}
              aria-label="Received into"
            >
              {assetWallets.length === 0 ? <option value="">No asset accounts</option> : null}
              {assetWallets.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </>
        ) : null}

        {kind === 'bill' ? (
          <>
            <select
              className={cn(compactControl, 'w-[4.25rem] shrink-0')}
              value={billStatus}
              onChange={(e) => setBillStatus(e.target.value as BillStatusTray)}
              disabled={busy}
              aria-label="Bill status"
            >
              <option value="unpaid">Unpaid</option>
              <option value="paid">Paid</option>
            </select>
            <select
              className={cn(compactControl, 'flex-1')}
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
              required
              disabled={busy}
              aria-label="Bill category"
            >
              {expenseAccounts.length === 0 ? <option value="">No expense accounts</option> : null}
              {expenseAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            {billStatus === 'paid' ? (
              <select
                className={cn(compactControl, 'flex-1')}
                value={walletId}
                onChange={(e) => setWalletId(e.target.value)}
                required
                disabled={busy}
                aria-label="Paid from"
              >
                {walletAccounts.length === 0 ? <option value="">No wallet accounts</option> : null}
                {walletAccounts.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
            ) : (
              <select
                className={cn(compactControl, 'flex-1')}
                value={payableId}
                onChange={(e) => setPayableId(e.target.value)}
                required
                disabled={busy}
                aria-label="Bills payable"
              >
                {payableAccounts.length === 0 ? (
                  <option value="">No liability accounts</option>
                ) : null}
                {payableAccounts.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
            )}
          </>
        ) : null}

        {kind === 'transfer' ? (
          <>
            <select
              className={cn(compactControl, 'flex-1')}
              value={fromId}
              onChange={(e) => setFromId(e.target.value)}
              required
              disabled={busy}
              aria-label="From"
            >
              {transferAccounts.length === 0 ? <option value="">No asset accounts</option> : null}
              {transferAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            <select
              className={cn(compactControl, 'flex-1')}
              value={toId}
              onChange={(e) => setToId(e.target.value)}
              required
              disabled={busy}
              aria-label="To"
            >
              {transferAccounts.length === 0 ? <option value="">No asset accounts</option> : null}
              {transferAccounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </>
        ) : null}

        <div className="w-[5.75rem] shrink-0 [&_input]:h-8 [&_input]:px-2 [&_input]:pr-8 [&_input]:text-xs [&_button]:h-7 [&_button]:w-7">
          <DateInput value={date} onChange={setDate} required aria-label="Entry date" />
        </div>

        <Button type="submit" size="sm" busy={busy} className="h-8 shrink-0 px-2.5">
          Add
        </Button>
      </div>

      {/* Memo + optional entity */}
      <div className="flex min-w-0 items-center gap-1">
        <Input
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Memo (optional)"
          className="h-8 flex-1 px-2 text-xs"
          aria-label="Memo"
          disabled={busy}
        />
        {entities.length > 1 ? (
          <select
            className={cn(compactControl, 'w-[7.5rem] shrink-0')}
            value={entity.id}
            onChange={(e) => void onEntityChange(e.target.value)}
            disabled={busy}
            aria-label="Book"
          >
            {entities.map((e) => (
              <option key={e.id} value={e.id}>
                {e.name}
              </option>
            ))}
          </select>
        ) : null}
      </div>

      <ErrorBanner message={error} className="mb-0 px-2 py-1 text-xs" />
    </form>
  )
}
