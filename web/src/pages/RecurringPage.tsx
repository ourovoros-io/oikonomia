import { useEffect, useId, useMemo, useRef, useState, type FormEvent } from 'react'
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
  type AccountDefaults,
  type Entity,
  type RecurringCadence,
  type RecurringKind,
  type RecurringTemplate,
  type SimpleEntryAccounts,
} from '../lib/api'
import { parseMajorToMinor } from '../lib/amountParse'
import { amountProblemKey, bookCurrency, minorToInputText } from '../lib/money'
import { beginExclusive } from '../lib/guards'
import { useDialogError } from '../lib/useDialogError'
import {
  accountsOf,
  formBillStatus,
  kindDefaultAccounts,
  type BillStatus,
} from '../lib/simpleEntry'
import { BillStatusFields, IncomeStatusFields } from '../components/StatusFields'
import {
  cadenceLabelKey,
  dayOfMonthForCadence,
  firstOnOrAfterMonthDay,
  firstOnOrAfterWeekday,
  formCadenceLabelKey,
  isRecurringCadence,
  kindBadgeTone,
  kindLabelKey,
  maxDayOfMonth,
  monthAndDayOfIso,
  weekdayOfIso,
} from '../lib/recurring'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DateInput } from '../components/DateInput'
import { Modal } from '../components/Modal'
import { RecurringCadenceFields } from '../components/RecurringCadenceFields'
import { TopBar } from '../components/TopBar'
import {
  Button,
  EmptyState,
  ErrorBanner,
  Field,
  IconBadge,
  Input,
  Panel,
  Segmented,
  Select,
  ToastStack,
} from '../components/ui'
import { cn } from '../lib/cn'
import { commandFailure, copyError } from '../lib/uiError'
import { useI18n } from '../lib/I18nProvider'
import { tn } from '../lib/i18n'

type Props = {
  entity: Entity
  onBack: () => void
}

const CADENCES: RecurringCadence[] = ['monthly', 'weekly', 'yearly']

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
        'inline-flex h-5 shrink-0 items-center rounded-full px-2 text-[11px] font-medium',
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
  const payExistingHintId = useId()
  const [templates, setTemplates] = useState<RecurringTemplate[]>([])
  const [accounts, setAccounts] = useState<Account[]>([])
  // Which account plays which role by default; Rust decides, this only holds it.
  const [defaults, setDefaults] = useState<AccountDefaults | null>(null)
  const [showForm, setShowForm] = useState(false)
  const [editId, setEditId] = useState<string | null>(null)
  const [kind, setKind] = useState<RecurringKind>('expense')
  const [billStatus, setBillStatus] = useState<BillStatus>('paid')
  // An income invoiced and not yet received, owed on the receivable account.
  const [incomeUnpaid, setIncomeUnpaid] = useState(false)
  const [name, setName] = useState('')
  const [amount, setAmount] = useState('')
  const [cadence, setCadence] = useState<RecurringCadence>('monthly')
  const [dayOfMonth, setDayOfMonth] = useState('1')
  // Where a weekly or yearly template recurs. Core keeps only `next_date` and
  // steps from it, so these pickers decide the date the template starts on.
  const [weekday, setWeekday] = useState(String(weekdayOfIso(todayISO())))
  const [yearMonth, setYearMonth] = useState(String(monthAndDayOfIso(todayISO()).month))
  const [yearDay, setYearDay] = useState(String(monthAndDayOfIso(todayISO()).day))
  const [categoryId, setCategoryId] = useState('')
  const [walletId, setWalletId] = useState('')
  // Bills payable, or the receivable of an unpaid income.
  const [payableId, setPayableId] = useState('')
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
  // A failure is drawn in the dialog that is open, not on the page behind its scrim.
  const openDialog = showForm ? 'form' : posting ? 'post' : deleting ? 'delete' : null
  const [error, setError] = useDialogError(openDialog)
  const dismissError = () => setError(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const formBusyRef = useRef(false)
  const postBusyRef = useRef(false)
  const deleteBusyRef = useRef(false)

  const ccy = entity.base_currency
  const currency = bookCurrency(entity)
  const accountMap = useMemo(() => new Map(accounts.map((a) => [a.id, a])), [accounts])
  const dueCount = templates.filter((row) => row.due).length

  const expenseAccounts = useMemo(() => accountsOf(accounts, ['expense']), [accounts])
  const incomeAccounts = useMemo(() => accountsOf(accounts, ['income']), [accounts])
  const walletAccounts = useMemo(() => accountsOf(accounts, ['asset', 'liability']), [accounts])
  const payableAccounts = useMemo(() => accountsOf(accounts, ['liability']), [accounts])
  const assetAccounts = useMemo(() => accountsOf(accounts, ['asset']), [accounts])

  function applyKindDefaults(nextKind: RecurringKind, roles: AccountDefaults | null) {
    if (nextKind === 'transfer') {
      const picked = kindDefaultAccounts('transfer', roles)
      setFromId(picked.fromId)
      setToId(picked.toId)
      return
    }

    // A recurring expense is booked like a bill: the same category default.
    const picked = kindDefaultAccounts(nextKind === 'income' ? 'income' : 'bill', roles)
    setCategoryId(picked.categoryId)
    setWalletId(picked.walletId)
    setPayableId(picked.payableId)
    setBillStatus('paid')
    setIncomeUnpaid(false)
  }

  /**
   * Fills the form from the kind, status and accounts Rust returned for a
   * template, which name its shape exactly. The kind's defaults go first, so
   * a part the template does not have holds the default, as in New entry.
   */
  function fillAccounts(form: SimpleEntryAccounts) {
    setKind(form.kind)
    applyKindDefaults(form.kind, defaults)
    if (form.kind === 'bill' && form.bill_status) setBillStatus(form.bill_status)
    setIncomeUnpaid(form.kind === 'income' && form.bill_status === 'unpaid')
    if (form.category_account_id) setCategoryId(form.category_account_id)
    if (form.wallet_account_id) setWalletId(form.wallet_account_id)
    if (form.payable_account_id) setPayableId(form.payable_account_id)
    if (form.from_account_id) setFromId(form.from_account_id)
    if (form.to_account_id) setToId(form.to_account_id)
  }

  async function reload() {
    const [rows, list, roles] = await Promise.all([
      api.recurringList(entity.id),
      api.accountList(entity.id),
      api.accountDefaults(entity.id),
    ])
    setTemplates(rows)
    setAccounts(list)
    setDefaults(roles)
  }

  useEffect(() => {
    void reload().catch((err) => setError(commandFailure(err)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entity.id])

  /** Points the weekday and yearly pickers at `iso`, without moving the start date. */
  function syncPickers(iso: string) {
    const { month, day } = monthAndDayOfIso(iso)
    setWeekday(String(weekdayOfIso(iso)))
    setYearMonth(String(month))
    setYearDay(String(day))
  }

  function pickWeekday(value: string) {
    setWeekday(value)
    setNextDate(firstOnOrAfterWeekday(todayISO(), Number(value)))
  }

  function pickYearly(month: string, day: string) {
    // Moving to a shorter month pulls the day back to the month's last.
    const clamped = Math.min(Number(day), maxDayOfMonth(Number(month)))
    setYearMonth(month)
    setYearDay(String(clamped))
    setNextDate(firstOnOrAfterMonthDay(todayISO(), Number(month), clamped))
  }

  function pickCadence(next: RecurringCadence) {
    setCadence(next)
    syncPickers(nextDate)
  }

  function resetForm() {
    setEditId(null)
    setKind('expense')
    setName('')
    setAmount('')
    setCadence('monthly')
    setDayOfMonth('1')
    setMemo('')
    setNextDate(todayISO())
    syncPickers(todayISO())
    applyKindDefaults('expense', defaults)
  }

  function openNew() {
    resetForm()
    setShowForm(true)
  }

  function openEdit(row: RecurringTemplate) {
    setEditId(row.id)
    fillAccounts(row)
    setName(row.name)
    setAmount(minorToInputText(row.amount_minor, currency))
    setCadence(row.cadence)
    setDayOfMonth(String(row.day_of_month ?? 1))
    setMemo(row.memo ?? '')
    setNextDate(row.next_date)
    syncPickers(row.next_date)
    setShowForm(true)
  }

  function closeForm() {
    if (formBusy) return
    setShowForm(false)
    setEditId(null)
  }

  function setKindAndDefaults(next: RecurringKind) {
    setKind(next)
    applyKindDefaults(next, defaults)
  }

  function accountSummary(row: RecurringTemplate): string {
    if (row.kind === 'transfer') {
      const from = accountMap.get(row.from_account_id ?? '')?.name ?? '—'
      const to = accountMap.get(row.to_account_id ?? '')?.name ?? '—'
      return `${from} → ${to}`
    }
    // Rust returns only the two accounts the template posts to; the others are null.
    const named = [row.category_account_id, row.payable_account_id, row.wallet_account_id]
      .map((id) => accountMap.get(id ?? '')?.name)
      .filter(Boolean)
    return named.join(' → ') || '—'
  }

  async function onSave(ev: FormEvent) {
    ev.preventDefault()
    if (!beginExclusive(formBusyRef)) return
    const minor = parseMajorToMinor(amount, currency)
    const problem = !name.trim()
      ? 'form.fieldRequired'
      : !nextDate
        ? 'date.invalid'
        : amountProblemKey(minor, 'tx.invalidAmount')
    // minor is only null alongside a problem; the check narrows its type.
    if (problem || minor === null) {
      formBusyRef.current = false
      setError(copyError(problem ?? 'tx.invalidAmount'))
      return
    }
    const day = dayOfMonthForCadence(cadence, Number(dayOfMonth))
    if (cadence === 'monthly' && day == null) {
      formBusyRef.current = false
      setError(copyError('recurring.form.error'))
      return
    }
    setFormBusy(true)
    setError(null)
    try {
      // Every account the form holds goes to Rust, which keeps the two the
      // template's kind and status post to, as it does for New entry.
      const shared = {
        name: name.trim(),
        kind,
        bill_status: formBillStatus(kind, billStatus, incomeUnpaid),
        amount_minor: minor,
        cadence,
        day_of_month: day,
        memo: memo.trim() || null,
        next_date: nextDate,
        category_account_id: categoryId || null,
        wallet_account_id: walletId || null,
        payable_account_id: payableId || null,
        from_account_id: fromId || null,
        to_account_id: toId || null,
      }
      if (editId) await api.recurringUpdate({ id: editId, ...shared })
      else await api.recurringCreate({ entity_id: entity.id, ...shared })
      setShowForm(false)
      setEditId(null)
      await reload()
    } catch (err) {
      setError(commandFailure(err, 'recurring.form.error'))
    } finally {
      formBusyRef.current = false
      setFormBusy(false)
    }
  }

  function openPost(row: RecurringTemplate) {
    setPosting(row)
    setPostDate(row.next_date || todayISO())
    setPostAmount(minorToInputText(row.amount_minor, currency))
  }

  async function confirmPost() {
    if (!posting || !beginExclusive(postBusyRef)) return
    const minor = parseMajorToMinor(postAmount, currency)
    if (minor === null || minor <= 0) {
      postBusyRef.current = false
      setError(copyError('recurring.posting.error'))
      return
    }
    setPostBusy(true)
    setError(null)
    try {
      await api.recurringPost(posting.id, { entry_date: postDate, amount_minor: minor })
      setPosting(null)
      await reload()
    } catch (err) {
      setError(commandFailure(err, 'recurring.posting.error'))
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
      setError(commandFailure(err))
    } finally {
      deleteBusyRef.current = false
      setDeleteBusy(false)
    }
  }

  const newButton = (
    <Button onClick={openNew}>
      <Plus className="size-4" />
      {t('recurring.new')}
    </Button>
  )

  const listMeta =
    templates.length === 0
      ? `${tn('recurring.templatesCount', 0)} · ${t('recurring.whisper')}`
      : `${tn('recurring.templatesCount', templates.length)} · ${tn('recurring.dueCount', dueCount)} · ${ccy}`

  return (
    <div className="space-y-4">
      <TopBar
        title={t('recurring.title')}
        subtitle={t('recurring.subtitle')}
        actions={
          <>
            <Button variant="secondary" onClick={onBack}>
              {t('recurring.backToEntries')}
            </Button>
            {newButton}
          </>
        }
      />

      <ToastStack>
        <ErrorBanner className="" error={openDialog ? null : error} onDismiss={dismissError} />
      </ToastStack>

      <Modal
        open={showForm}
        title={editId ? t('recurring.form.titleEdit') : t('recurring.form.titleNew')}
        description={t('recurring.subtitle')}
        error={openDialog === 'form' ? error : null}
        onDismissError={dismissError}
        onClose={closeForm}
      >
        <div className="mb-5">
          <p className="mb-1.5 font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
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
                tone: 'money-out',
              },
              {
                id: 'income',
                label: t('tx.form.kind.income'),
                icon: <KindTile kind="income" size="xs" />,
                tone: 'money-in',
              },
              {
                id: 'bill',
                label: t('tx.form.kind.bill'),
                icon: <KindTile kind="bill" size="xs" />,
                tone: 'money-out',
              },
              {
                id: 'transfer',
                label: t('tx.form.kind.transfer'),
                icon: <KindTile kind="transfer" size="xs" />,
              },
            ]}
          />
        </div>

        <form noValidate onSubmit={onSave} className="grid gap-4 sm:grid-cols-2">
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
                if (isRecurringCadence(e.target.value)) pickCadence(e.target.value)
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
          <RecurringCadenceFields
            cadence={cadence}
            dayOfMonth={dayOfMonth}
            weekday={weekday}
            yearMonth={yearMonth}
            yearDay={yearDay}
            nextDate={nextDate}
            onDayOfMonth={setDayOfMonth}
            onWeekday={pickWeekday}
            onYearly={pickYearly}
          />
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
          ) : null}
          {kind === 'expense' ? (
            <>
              <Field label={t('recurring.form.category')}>
                <Select value={categoryId} onChange={(e) => setCategoryId(e.target.value)} required>
                  {expenseAccounts.map((a) => (
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
          ) : null}
          {kind === 'income' ? (
            <IncomeStatusFields
              unpaid={incomeUnpaid}
              onUnpaid={setIncomeUnpaid}
              category={{ value: categoryId, onChange: setCategoryId, options: incomeAccounts }}
              receivable={{ value: payableId, onChange: setPayableId, options: assetAccounts }}
              wallet={{ value: walletId, onChange: setWalletId, options: assetAccounts }}
              wide="sm:col-span-2"
            />
          ) : null}
          {kind === 'bill' ? (
            <BillStatusFields
              status={billStatus}
              onStatus={setBillStatus}
              statusDescribedBy={billStatus === 'pay_existing' ? payExistingHintId : undefined}
              category={{ value: categoryId, onChange: setCategoryId, options: expenseAccounts }}
              wallet={{ value: walletId, onChange: setWalletId, options: walletAccounts }}
              payable={{ value: payableId, onChange: setPayableId, options: payableAccounts }}
              payment={
                <p
                  id={payExistingHintId}
                  className="text-xs text-[var(--color-muted)] sm:col-span-2"
                >
                  {t('recurring.form.payExistingHint')}
                </p>
              }
              wide="sm:col-span-2"
            />
          ) : null}
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
        <ErrorBanner
          error={openDialog === 'post' ? error : null}
          className="mb-4"
          onDismiss={dismissError}
        />
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
              <dt className="text-[var(--color-muted)]">{t('recurring.postConfirm.accounts')}</dt>
              <dd className="text-[var(--color-fg)]">{accountSummary(posting)}</dd>
              <dt className="text-[var(--color-muted)]">{t('recurring.form.kind')}</dt>
              <dd className="text-[var(--color-fg)]">{t(kindLabelKey(posting.kind))}</dd>
            </dl>
            {posting.kind === 'bill' && posting.bill_status === 'pay_existing' ? (
              <p className="mt-3 text-xs text-[var(--color-muted)]">
                {t('recurring.form.payExistingHint')}
              </p>
            ) : null}
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
      >
        <ErrorBanner
          error={openDialog === 'delete' ? error : null}
          className=""
          onDismiss={dismissError}
        />
      </ConfirmDialog>

      <Panel
        title={t('recurring.templates')}
        description={listMeta}
        whisper={templates.length > 0 ? t('recurring.whisper') : undefined}
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
                <li key={row.id} className="flex items-center gap-4 px-5 py-3">
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
                      income ? 'text-[var(--color-money-in-text)]' : 'text-[var(--color-fg)]',
                    )}
                  >
                    {formatMoney(row.amount_minor, currency, undefined, { signed: income })}
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
                    size="iconSm"
                    onClick={() => openEdit(row)}
                    aria-label={t('recurring.form.titleEdit')}
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
