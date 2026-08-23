import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react'
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  FileText,
  Pencil,
  Plus,
  Repeat,
} from 'lucide-react'
import {
  api,
  formatDate,
  formatMoney,
  todayISO,
  type Account,
  type Entity,
  type RecurringCadence,
  type RecurringKind,
  type RecurringTemplate,
} from '../lib/api'
import { currencyFractionDigits, parseMajorToMinor } from '../lib/money'
import { beginExclusive } from '../lib/guards'
import {
  billStatusForKind,
  cadenceLabelKey,
  dayOfMonthForCadence,
  formCadenceLabelKey,
  isRecurringCadence,
  kindBadgeTone,
  kindLabelKey,
  recurringAccountIds,
} from '../lib/recurring'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DateInput } from '../components/DateInput'
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
import { useI18n } from '../lib/I18nProvider'

type Props = {
  entity: Entity
  onBack: () => void
}

const CADENCES: RecurringCadence[] = ['monthly', 'weekly', 'yearly']

function accountsOf(accounts: Account[], types: Account['account_type'][]): Account[] {
  return accounts.filter((a) => a.is_active && types.includes(a.account_type))
}

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

function majorString(minor: number, currency: string): string {
  const digits = currencyFractionDigits(currency)
  return (minor / 10 ** digits).toFixed(digits)
}

function KindIcon({ kind, className }: { kind: RecurringKind; className: string }) {
  if (kind === 'income') return <ArrowDownLeft className={className} />
  if (kind === 'bill') return <FileText className={className} />
  if (kind === 'transfer') return <ArrowLeftRight className={className} />
  return <ArrowUpRight className={className} />
}

function KindTile({
  kind,
  size = 'md',
}: {
  kind: RecurringKind
  size?: 'xs' | 'md'
}) {
  return (
    <IconBadge tone={kindBadgeTone(kind)} size={size}>
      <KindIcon kind={kind} className={size === 'xs' ? 'size-3.5' : 'size-4'} />
    </IconBadge>
  )
}

function Pill({
  children,
  tone = 'muted',
}: {
  children: string
  tone?: 'muted' | 'warning'
}) {
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center rounded-full px-1.5 py-px text-[10px] font-medium leading-4',
        tone === 'warning'
          ? 'bg-[var(--color-warning-soft)] text-[var(--color-warning)]'
          : 'bg-[var(--color-surface-elevated)] text-[var(--color-muted)]',
      )}
    >
      {children}
    </span>
  )
}

export function RecurringPage({ entity, onBack }: Props) {
  const { t } = useI18n()
  const [templates, setTemplates] = useState<RecurringTemplate[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])
  const [error, setError] = useState<string | null>(null)
  const [showForm, setShowForm] = useState(false)
  const [editId, setEditId] = useState<string | null>(null)
  const [kind, setKind] = useState<RecurringKind>('expense')
  const [name, setName] = useState('')
  const [amount, setAmount] = useState('')
  const [cadence, setCadence] = useState<RecurringCadence>('monthly')
  const [dayOfMonth, setDayOfMonth] = useState('1')
  const [categoryId, setCategoryId] = useState('')
  const [walletId, setWalletId] = useState('')
  const [fromId, setFromId] = useState('')
  const [toId, setToId] = useState('')
  const [memo, setMemo] = useState('')
  const [nextDate, setNextDate] = useState(todayISO())
  const [formBusy, setFormBusy] = useState(false)
  const [posting, setPosting] = useState<RecurringTemplate | null>(null)
  const [postDate, setPostDate] = useState(todayISO())
  const [postAmount, setPostAmount] = useState('')
  const [postBusy, setPostBusy] = useState(false)
  const [deleting, setDeleting] = useState<RecurringTemplate | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const formBusyRef = useRef(false)
  const postBusyRef = useRef(false)
  const deleteBusyRef = useRef(false)

  const ccy = entity.base_currency
  const accountMap = useMemo(() => new Map(accounts.map((a) => [a.id, a])), [accounts])
  const dueCount = templates.filter((row) => row.due).length

  const expenseAccounts = useMemo(() => accountsOf(accounts, ['expense']), [accounts])
  const incomeAccounts = useMemo(() => accountsOf(accounts, ['income']), [accounts])
  const walletAccounts = useMemo(() => accountsOf(accounts, ['asset', 'liability']), [accounts])
  const categoryAccounts = kind === 'income' ? incomeAccounts : expenseAccounts

  function applyKindDefaults(nextKind: RecurringKind, list: Account[]) {
    if (nextKind === 'income') {
      setCategoryId(pickDefault(list, 'income', ['salary', 'sales', 'freelance']))
      setWalletId(pickDefault(list, 'asset', ['checking', 'bank', 'cash']))
    } else if (nextKind === 'transfer') {
      setFromId(pickDefault(list, 'asset', ['checking', 'bank']))
      const savings = pickDefault(list, 'asset', ['savings', 'cash'])
      setToId(savings || pickDefault(list, 'asset', []))
    } else {
      setCategoryId(
        pickDefault(list, 'expense', ['rent', 'utilities', 'bills', 'subscription', 'housing']),
      )
      setWalletId(pickDefault(list, 'asset', ['checking', 'bank', 'cash']))
    }
  }

  async function reload() {
    const [rows, list] = await Promise.all([
      api.recurringList(entity.id),
      api.accountList(entity.id),
    ])
    setTemplates(rows)
    setAccounts(list)
  }

  useEffect(() => {
    void reload().catch((err) => setError((err as CommandError).message))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity.id])

  function resetForm() {
    setEditId(null)
    setKind('expense')
    setName('')
    setAmount('')
    setCadence('monthly')
    setDayOfMonth('1')
    setMemo('')
    setNextDate(todayISO())
    applyKindDefaults('expense', accounts)
  }

  function openNew() {
    resetForm()
    setShowForm(true)
  }

  function openEdit(row: RecurringTemplate) {
    setEditId(row.id)
    setKind(row.kind)
    setName(row.name)
    setAmount(majorString(row.amount_minor, ccy))
    setCadence(row.cadence)
    setDayOfMonth(String(row.day_of_month ?? 1))
    setCategoryId(row.category_account_id ?? '')
    setWalletId(row.wallet_account_id ?? '')
    setFromId(row.from_account_id ?? '')
    setToId(row.to_account_id ?? '')
    setMemo(row.memo ?? '')
    setNextDate(row.next_date)
    setShowForm(true)
  }

  function closeForm() {
    if (formBusy) return
    setShowForm(false)
    setEditId(null)
  }

  function setKindAndDefaults(next: RecurringKind) {
    setKind(next)
    applyKindDefaults(next, accounts)
  }

  function accountSummary(row: RecurringTemplate): string {
    if (row.kind === 'transfer') {
      const from = accountMap.get(row.from_account_id ?? '')?.name ?? '—'
      const to = accountMap.get(row.to_account_id ?? '')?.name ?? '—'
      return `${from} → ${to}`
    }
    const category = accountMap.get(row.category_account_id ?? '')?.name
    const wallet = accountMap.get(row.wallet_account_id ?? '')?.name
    return [category, wallet].filter(Boolean).join(' → ') || '—'
  }

  async function onSave(ev: FormEvent) {
    ev.preventDefault()
    if (!beginExclusive(formBusyRef)) return
    const minor = parseMajorToMinor(amount, ccy)
    if (minor === null || minor <= 0 || !name.trim()) {
      formBusyRef.current = false
      setError(t('recurring.form.error'))
      return
    }
    const day = dayOfMonthForCadence(cadence, Number(dayOfMonth))
    if (cadence === 'monthly' && day == null) {
      formBusyRef.current = false
      setError(t('recurring.form.error'))
      return
    }
    setFormBusy(true)
    setError(null)
    try {
      const roleIds = recurringAccountIds(kind, {
        categoryId,
        walletId,
        fromId,
        toId,
      })
      const shared = {
        name: name.trim(),
        kind,
        bill_status: billStatusForKind(kind),
        amount_minor: minor,
        cadence,
        day_of_month: day,
        memo: memo.trim() || null,
        next_date: nextDate,
        ...roleIds,
      }
      if (editId) await api.recurringUpdate({ id: editId, ...shared })
      else await api.recurringCreate({ entity_id: entity.id, ...shared })
      setShowForm(false)
      setEditId(null)
      await reload()
    } catch (err) {
      setError((err as CommandError).message || t('recurring.form.error'))
    } finally {
      formBusyRef.current = false
      setFormBusy(false)
    }
  }

  function openPost(row: RecurringTemplate) {
    setPosting(row)
    setPostDate(row.next_date || todayISO())
    setPostAmount(majorString(row.amount_minor, ccy))
  }

  async function confirmPost() {
    if (!posting || !beginExclusive(postBusyRef)) return
    const minor = parseMajorToMinor(postAmount, ccy)
    if (minor === null || minor <= 0) {
      postBusyRef.current = false
      setError(t('recurring.posting.error'))
      return
    }
    setPostBusy(true)
    setError(null)
    try {
      await api.recurringPost(posting.id, { entry_date: postDate, amount_minor: minor })
      setPosting(null)
      await reload()
    } catch (err) {
      setError((err as CommandError).message || t('recurring.posting.error'))
    } finally {
      postBusyRef.current = false
      setPostBusy(false)
    }
  }

  async function confirmDelete() {
    if (!deleting || !beginExclusive(deleteBusyRef)) return
    setDeleteBusy(true)
    setError(null)
    try {
      await api.recurringDelete(deleting.id)
      setDeleting(null)
      setShowForm(false)
      setEditId(null)
      await reload()
    } catch (err) {
      setError((err as CommandError).message)
    } finally {
      deleteBusyRef.current = false
      setDeleteBusy(false)
    }
  }

  const newButton = (
    <Button size="sm" onClick={openNew}>
      <Plus className="size-3" />
      {t('recurring.new')}
    </Button>
  )

  const listMeta =
    templates.length === 0
      ? `${t('recurring.templatesCount', { n: 0 })} · ${t('recurring.whisper')}`
      : `${t('recurring.templatesCount', { n: templates.length })} · ${t('recurring.dueCount', { n: dueCount })} · ${ccy}`

  return (
    <div className="space-y-6">
      <PageHeader
        breadcrumb={[
          { label: t('tx.title'), onClick: onBack },
          { label: t('recurring.title') },
        ]}
        title={t('recurring.title')}
        description={t('recurring.subtitle')}
        actions={newButton}
      />

      <ErrorBanner message={error} />

      <Modal
        open={showForm}
        title={editId ? t('recurring.form.titleEdit') : t('recurring.form.titleNew')}
        description={t('recurring.subtitle')}
        onClose={closeForm}
      >
        <div className="mb-5">
          <p className="mb-1.5 text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
            {t('recurring.form.kind')}
          </p>
          <Segmented<RecurringKind>
            value={kind}
            onChange={setKindAndDefaults}
            className="h-11"
            options={[
              {
                id: 'expense',
                label: t('tx.form.kind.expense'),
                icon: <KindTile kind="expense" size="xs" />,
              },
              {
                id: 'income',
                label: t('tx.form.kind.income'),
                icon: <KindTile kind="income" size="xs" />,
              },
              {
                id: 'bill',
                label: t('tx.form.kind.bill'),
                icon: <KindTile kind="bill" size="xs" />,
              },
              {
                id: 'transfer',
                label: t('tx.form.kind.transfer'),
                icon: <KindTile kind="transfer" size="xs" />,
              },
            ]}
          />
        </div>

        <form onSubmit={onSave} className="grid gap-4 sm:grid-cols-2">
          <Field label={t('recurring.form.name')} className="sm:col-span-2">
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('recurring.form.namePlaceholder')}
              required
            />
          </Field>
          <Field label={t('recurring.form.amount', { currency: ccy })}>
            <Input
              inputMode="decimal"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              className="tabular-nums"
              aria-label={t('recurring.form.amount', { currency: ccy })}
              required
            />
          </Field>
          <Field label={t('recurring.form.cadence')}>
            <Select
              value={cadence}
              onChange={(e) => {
                if (isRecurringCadence(e.target.value)) setCadence(e.target.value)
              }}
              aria-label={t('recurring.form.cadence')}
            >
              {CADENCES.map((id) => (
                <option key={id} value={id}>
                  {t(formCadenceLabelKey(id))}
                </option>
              ))}
            </Select>
          </Field>
          {cadence === 'monthly' ? (
            <Field label={t('recurring.form.dayOfMonth')}>
              <Select
                value={dayOfMonth}
                onChange={(e) => setDayOfMonth(e.target.value)}
                aria-label={t('recurring.form.dayOfMonth')}
              >
                {Array.from({ length: 31 }, (_, i) => String(i + 1)).map((day) => (
                  <option key={day} value={day}>
                    {day}
                  </option>
                ))}
              </Select>
              <p className="mt-1.5 text-xs text-[var(--color-muted)]">
                {t('recurring.form.dayOfMonthHint')}
              </p>
            </Field>
          ) : null}
          {kind === 'transfer' ? (
            <>
              <Field label={t('recurring.form.fromAccount')}>
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
          ) : (
            <>
              <Field label={t('recurring.form.category')}>
                <Select value={categoryId} onChange={(e) => setCategoryId(e.target.value)} required>
                  {categoryAccounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.code} · {a.name}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label={t('recurring.form.fromAccount')}>
                <Select value={walletId} onChange={(e) => setWalletId(e.target.value)} required>
                  {walletAccounts.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.code} · {a.name}
                    </option>
                  ))}
                </Select>
              </Field>
            </>
          )}
          <Field label={t('recurring.form.memo')} className="sm:col-span-2">
            <Input
              value={memo}
              onChange={(e) => setMemo(e.target.value)}
              placeholder={t('recurring.form.memoPlaceholder')}
            />
          </Field>
          <div className="flex justify-end gap-2 border-t border-[var(--color-border)] pt-4 sm:col-span-2">
            {editId ? (
              <Button
                type="button"
                variant="danger"
                disabled={formBusy}
                className="mr-auto"
                onClick={() => {
                  const row = templates.find((item) => item.id === editId)
                  if (row) setDeleting(row)
                }}
              >
                {t('recurring.delete.confirm')}
              </Button>
            ) : null}
            <Button type="button" variant="secondary" disabled={formBusy} onClick={closeForm}>
              {t('recurring.form.cancel')}
            </Button>
            <Button type="submit" busy={formBusy}>
              {formBusy ? t('recurring.form.busy') : t('recurring.form.save')}
            </Button>
          </div>
        </form>
      </Modal>

      <ConfirmDialog
        open={posting !== null}
        title={t('recurring.postConfirm.title', { name: posting?.name ?? '' })}
        body={t('recurring.postConfirm.body')}
        confirmLabel={t('recurring.postConfirm.confirm')}
        cancelLabel={t('recurring.postConfirm.cancel')}
        busyLabel={t('recurring.posting.busy')}
        tone="success"
        busy={postBusy}
        onCancel={() => {
          if (!postBusy) setPosting(null)
        }}
        onConfirm={() => void confirmPost()}
      >
        {posting ? (
          <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] px-4 py-3">
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label={t('tx.date')}>
                <DateInput
                  value={postDate}
                  onChange={setPostDate}
                  required
                  aria-label={t('tx.date')}
                />
              </Field>
              <Field label={t('recurring.form.amount', { currency: ccy })}>
                <Input
                  inputMode="decimal"
                  value={postAmount}
                  onChange={(e) => setPostAmount(e.target.value)}
                  className="tabular-nums"
                  aria-label={t('recurring.form.amount', { currency: ccy })}
                  required
                />
              </Field>
            </div>
            <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-sm">
              <dt className="text-[var(--color-muted)]">{t('recurring.form.fromAccount')}</dt>
              <dd className="text-[var(--color-fg)]">{accountSummary(posting)}</dd>
              <dt className="text-[var(--color-muted)]">{t('recurring.form.kind')}</dt>
              <dd className="text-[var(--color-fg)]">{t(kindLabelKey(posting.kind))}</dd>
            </dl>
          </div>
        ) : null}
      </ConfirmDialog>

      <ConfirmDialog
        open={deleting !== null}
        title={t('recurring.delete.title')}
        body={t('recurring.delete.body', { name: deleting?.name ?? '' })}
        confirmLabel={t('recurring.delete.confirm')}
        danger
        busy={deleteBusy}
        onCancel={() => {
          if (!deleteBusy) setDeleting(null)
        }}
        onConfirm={() => void confirmDelete()}
      />

      <Panel
        title={t('recurring.templates')}
        description={listMeta}
        whisper={templates.length > 0 ? t('recurring.whisper') : undefined}
        actions={
          <Button variant="secondary" size="sm" onClick={onBack}>
            {t('recurring.backToEntries')}
          </Button>
        }
      >
        {templates.length === 0 ? (
          <div className="p-5">
            <EmptyState
              icon={<Repeat className="size-5" />}
              title={t('recurring.emptyTitle')}
              body={t('recurring.emptyBody')}
              action={newButton}
            />
          </div>
        ) : (
          <ul className="divide-y divide-[var(--color-border)]">
            {templates.map((row) => {
              const income = row.kind === 'income'
              return (
                <li key={row.id} className="flex items-center gap-4 px-5 py-3.5">
                  <KindTile kind={row.kind} />
                  <div className="min-w-0 flex-1">
                    <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                      <span className="truncate text-sm font-medium text-[var(--color-fg)]">
                        {row.name}
                      </span>
                      {row.due ? <Pill tone="warning">{t('recurring.due')}</Pill> : null}
                      <Pill>{t(kindLabelKey(row.kind))}</Pill>
                    </div>
                    <div className="truncate text-xs text-[var(--color-muted)]">
                      {[
                        t(cadenceLabelKey(row.cadence)),
                        row.cadence === 'monthly' && row.day_of_month != null
                          ? String(row.day_of_month)
                          : null,
                        accountSummary(row),
                        row.next_date ? formatDate(row.next_date) : null,
                      ]
                        .filter((part): part is string => Boolean(part))
                        .join(' · ')}
                    </div>
                  </div>
                  <div
                    className={cn(
                      'shrink-0 text-sm font-semibold tabular-nums',
                      income ? 'text-[var(--color-success)]' : 'text-[var(--color-fg)]',
                    )}
                  >
                    {formatMoney(row.amount_minor, ccy, undefined, { signed: income })}
                  </div>
                  <Button
                    variant={row.due ? 'primary' : 'ghost'}
                    size="sm"
                    onClick={() => openPost(row)}
                  >
                    {t('recurring.post')}
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    onClick={() => openEdit(row)}
                    aria-label={t('recurring.form.titleEdit')}
                    title={t('recurring.form.titleEdit')}
                  >
                    <Pencil className="size-4" />
                  </Button>
                </li>
              )
            })}
          </ul>
        )}
      </Panel>
    </div>
  )
}
