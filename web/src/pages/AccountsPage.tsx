import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react'
import {
  Banknote,
  CircleOff,
  Coins,
  CreditCard,
  Landmark,
  PencilLine,
  PieChart,
  Plus,
  Receipt,
  RotateCcw,
  TrendingDown,
  TrendingUp,
  Wallet,
  X,
} from 'lucide-react'
import {
  api,
  formatDate,
  formatMoney,
  todayISO,
  type Account,
  type AccountType,
  type Entity,
  type RegisterLine,
} from '../lib/api'
import { parseMajorToMinor } from '../lib/amountParse'
import { bookCurrency } from '../lib/money'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { DateInput } from '../components/DateInput'
import { HiddenBadge } from '../components/hiddenUi'
import { Modal } from '../components/Modal'
import { RenameDialog } from '../components/RenameDialog'
import { TopBar } from '../components/TopBar'
import {
  Button,
  Card,
  EmptyState,
  ErrorBanner,
  Field,
  IconBadge,
  Input,
  MetricCard,
  NoticeBanner,
  Panel,
  Select,
} from '../components/ui'
import { cn } from '../lib/cn'
import { commandErrorMessage } from '../lib/commandError'
import { useI18n } from '../lib/I18nProvider'

type Props = { entity: Entity | null; onCreateBook?: () => void }

const TYPES: Array<{
  id: AccountType
  labelKey:
    | 'accountType.asset'
    | 'accountType.liability'
    | 'accountType.equity'
    | 'accountType.income'
    | 'accountType.expense'
  icon: typeof Wallet
}> = [
  { id: 'asset', labelKey: 'accountType.asset', icon: Landmark },
  { id: 'liability', labelKey: 'accountType.liability', icon: CreditCard },
  { id: 'equity', labelKey: 'accountType.equity', icon: PieChart },
  { id: 'income', labelKey: 'accountType.income', icon: TrendingUp },
  { id: 'expense', labelKey: 'accountType.expense', icon: TrendingDown },
]

function typeMeta(t: AccountType) {
  return TYPES.find((x) => x.id === t) ?? TYPES[0]
}

/**
 * Money identity uses the Ledger, never status: income/expense wear
 * money-in/money-out. The balance-sheet types (asset, liability, equity)
 * are not money moving in or out, so none of them may wear success, danger
 * or warning; they instead each get their own non-status tone so they stay
 * distinguishable from one another.
 */
function typeTone(t: AccountType): 'accent' | 'info' | 'muted' | 'money-in' | 'money-out' {
  if (t === 'income') return 'money-in'
  if (t === 'expense') return 'money-out'
  if (t === 'asset') return 'accent'
  if (t === 'liability') return 'info'
  return 'muted'
}

/** Balance-sheet accounts have a balance as of today; income and expense are read in Reports. */
function showsBalance(account: Account): boolean {
  return (
    account.account_type === 'asset' ||
    account.account_type === 'liability' ||
    account.account_type === 'equity'
  )
}

/** The chart in code order, whatever order the accounts were created in. */
function byCode(a: Account, b: Account): number {
  return a.code.localeCompare(b.code, undefined, { numeric: true })
}

export function AccountsPage({ entity, onCreateBook }: Props) {
  const { t } = useI18n()
  const [accounts, setAccounts] = useState<Account[]>([])
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [balances, setBalances] = useState<Map<string, number>>(new Map())
  const [renameTarget, setRenameTarget] = useState<Account | null>(null)
  const [deactivateTarget, setDeactivateTarget] = useState<Account | null>(null)
  const [deactivateBusy, setDeactivateBusy] = useState(false)
  const codeInputRef = useRef<HTMLInputElement>(null)
  const [showForm, setShowForm] = useState(false)
  // Drawn inside the Add account card, where the person is looking.
  const [formError, setFormError] = useState<string | null>(null)
  const [code, setCode] = useState('')
  const [name, setName] = useState('')
  const [accountType, setAccountType] = useState<AccountType>('expense')
  const [busy, setBusy] = useState(false)
  const [balanceAccount, setBalanceAccount] = useState<Account | null>(null)
  const [balanceAmount, setBalanceAmount] = useState('')
  const [balanceAsOf, setBalanceAsOf] = useState(todayISO())
  const [balanceCurrent, setBalanceCurrent] = useState<number | null>(null)
  const [balanceError, setBalanceError] = useState<string | null>(null)
  const [balanceBusy, setBalanceBusy] = useState(false)
  const [registerAccount, setRegisterAccount] = useState<Account | null>(null)
  const [registerLines, setRegisterLines] = useState<RegisterLine[]>([])
  const [registerError, setRegisterError] = useState<string | null>(null)
  const [registerBusy, setRegisterBusy] = useState(false)

  async function reload() {
    if (!entity) return
    const list = await api.accountList(entity.id)
    setAccounts(list)

    const today = todayISO()
    const pairs = await Promise.all(
      list.filter(showsBalance).map(async (a) => [a.id, await api.accountBalance(a.id, today)] as const),
    )
    setBalances(new Map(pairs))
  }

  useEffect(() => {
    if (!entity) {
      setAccounts([])
      return
    }
    void reload().catch((err) => setError(commandErrorMessage(err)))
  }, [entity?.id])

  const sortedAccounts = useMemo(() => [...accounts].sort(byCode), [accounts])

  const counts = useMemo(() => {
    const active = accounts.filter((a) => a.is_active)
    return {
      total: active.length,
      assets: active.filter((a) => a.account_type === 'asset').length,
      income: active.filter((a) => a.account_type === 'income').length,
      expense: active.filter((a) => a.account_type === 'expense').length,
    }
  }, [accounts])

  async function onCreate(ev: FormEvent) {
    ev.preventDefault()
    if (!entity) return
    setBusy(true)
    setFormError(null)
    setNotice(null)
    try {
      await api.accountCreate({
        entity_id: entity.id,
        code,
        name,
        account_type: accountType,
      })
      // The form stays open for the next account; the type is kept because
      // accounts are usually added in runs of one type.
      setNotice(t('accounts.form.created', { code: code.trim(), name: name.trim() }))
      setCode('')
      setName('')
      codeInputRef.current?.focus()
      await reload()
    } catch (err) {
      setFormError(commandErrorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  async function confirmDeactivate() {
    if (!deactivateTarget) return
    setDeactivateBusy(true)
    setError(null)
    setNotice(null)
    try {
      await api.accountArchive(deactivateTarget.id)
      setNotice(t('accounts.deactivate.done', { name: deactivateTarget.name }))
      await reload()
    } catch (err) {
      setError(commandErrorMessage(err))
    } finally {
      // Closed on failure too, so the error is not behind the dialog.
      setDeactivateTarget(null)
      setDeactivateBusy(false)
    }
  }

  async function reactivate(account: Account) {
    setError(null)
    setNotice(null)
    try {
      await api.accountUpdate({
        id: account.id,
        code: account.code,
        name: account.name,
        is_active: true,
        sort_order: account.sort_order,
      })
      setNotice(t('accounts.reactivate.done', { name: account.name }))
      await reload()
    } catch (err) {
      setError(commandErrorMessage(err))
    }
  }

  async function rename(account: Account, name: string) {
    await api.accountUpdate({
      id: account.id,
      code: account.code,
      name,
      is_active: account.is_active,
      sort_order: account.sort_order,
    })
    await reload()
  }

  function openBalance(account: Account) {
    setBalanceAccount(account)
    setBalanceAmount('')
    setBalanceAsOf(todayISO())
    setBalanceCurrent(null)
    setBalanceError(null)
    void api
      .accountBalance(account.id, todayISO())
      .then(setBalanceCurrent)
      .catch(() => setBalanceCurrent(null))
  }

  function openRegister(account: Account) {
    setRegisterAccount(account)
    setRegisterLines([])
    setRegisterError(null)
  }

  useEffect(() => {
    if (!registerAccount) return
    let cancelled = false
    setRegisterBusy(true)
    setRegisterError(null)
    void api
      .accountRegister(registerAccount.id)
      .then((lines) => {
        if (!cancelled) setRegisterLines(lines)
      })
      .catch((err) => {
        if (!cancelled) setRegisterError(commandErrorMessage(err))
      })
      .finally(() => {
        if (!cancelled) setRegisterBusy(false)
      })
    return () => {
      cancelled = true
    }
  }, [registerAccount?.id])

  async function onSetBalance(ev: FormEvent) {
    ev.preventDefault()
    if (!balanceAccount || !entity) return

    if (!balanceAsOf) {
      setBalanceError(t('date.invalid'))
      return
    }

    const minor = parseMajorToMinor(balanceAmount, bookCurrency(entity))
    if (minor === null) {
      setBalanceError(
        t(balanceAccount.account_type === 'liability' ? 'accounts.balance.error.invalidOwed' : 'acct.invalidAmount'),
      )
      return
    }

    setBalanceBusy(true)
    setBalanceError(null)
    setNotice(null)
    try {
      await api.accountSetOpeningBalance(balanceAccount.id, minor, balanceAsOf)
      setNotice(
        t('accounts.balance.saved', {
          name: balanceAccount.name,
          amount: formatMoney(minor, bookCurrency(entity)),
        }),
      )
      setBalanceAccount(null)
      await reload()
    } catch (err) {
      setBalanceError(commandErrorMessage(err))
    } finally {
      setBalanceBusy(false)
    }
  }

  if (!entity) {
    return (
      <EmptyState
        icon={<Wallet className="size-5" />}
        title={t('acct.noBookTitle')}
        body={t('acct.noBookBody')}
        action={
          onCreateBook ? (
            <Button onClick={onCreateBook}>{t('empty.createBook')}</Button>
          ) : undefined
        }
      />
    )
  }

  if (registerAccount) {
    return (
      <div className="space-y-4">
        <TopBar
          title={t('accounts.register.title', { name: registerAccount.name })}
          subtitle={`${entity.name} · ${entity.base_currency}`}
          actions={
            <Button variant="secondary" onClick={() => setRegisterAccount(null)}>
              {t('accounts.register.back')}
            </Button>
          }
        />

        <ErrorBanner message={registerError} onDismiss={() => setRegisterError(null)} />

        <Panel title={registerAccount.name} icon={<Receipt className="size-4" />}>
          {registerBusy ? (
            <p className="px-5 py-10 text-center text-sm text-[var(--color-muted)]">
              {t('common.loading')}
            </p>
          ) : registerLines.length === 0 ? (
            <p className="px-5 py-10 text-center text-sm text-[var(--color-muted)]">
              {t('accounts.register.empty')}
            </p>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-[var(--color-border)] text-left text-xs text-[var(--color-muted)]">
                    <th className="px-5 py-2 font-medium">{t('tx.date')}</th>
                    <th className="px-5 py-2 font-medium">{t('tx.descriptionLabel')}</th>
                    <th className="px-5 py-2 text-right font-medium">{t('rpt.debit')}</th>
                    <th className="px-5 py-2 text-right font-medium">{t('rpt.credit')}</th>
                    <th className="px-5 py-2 text-right font-medium">
                      {t('accounts.register.balance')}
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-[var(--color-border)]">
                  {registerLines.map((line) => (
                    <tr key={line.entry_id} className={cn(line.hidden && 'opacity-50')}>
                      <td className="px-5 py-3 whitespace-nowrap text-sm text-[var(--color-fg-secondary)] tabular-nums">
                        {formatDate(line.entry_date)}
                      </td>
                      <td className="px-5 py-3">
                        <span className="text-[var(--color-fg)]">{line.description}</span>
                        {line.hidden ? <HiddenBadge className="ml-2" /> : null}
                      </td>
                      <td className="px-5 py-3 text-right tabular-nums text-[var(--color-fg)]">
                        {line.debit_minor ? formatMoney(line.debit_minor, bookCurrency(entity)) : ''}
                      </td>
                      <td className="px-5 py-3 text-right tabular-nums text-[var(--color-fg)]">
                        {line.credit_minor ? formatMoney(line.credit_minor, bookCurrency(entity)) : ''}
                      </td>
                      <td className="px-5 py-3 text-right font-medium tabular-nums text-[var(--color-fg)]">
                        {formatMoney(line.balance_minor, bookCurrency(entity))}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Panel>
      </div>
    )
  }

  const owes = balanceAccount?.account_type === 'liability'
  const enteredMinor = parseMajorToMinor(balanceAmount, bookCurrency(entity))

  return (
    <div className="space-y-4">
      <TopBar
        title={t('acct.title')}
        subtitle={`${entity.name} · ${entity.base_currency}`}
        actions={
          <Button
            onClick={() => {
              setFormError(null)
              setShowForm((v) => !v)
            }}
          >
            {showForm ? <X className="size-4" /> : <Plus className="size-4" />}
            {showForm ? t('acct.close') : t('acct.addAccount')}
          </Button>
        }
      />

      <ErrorBanner message={error} onDismiss={() => setError(null)} />
      <NoticeBanner message={notice} />

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <MetricCard
          label={t('acct.activeAccounts')}
          hint={t('acct.inThisBook')}
          value={String(counts.total)}
          icon={<Wallet className="size-4" />}
        />
        <MetricCard
          label={t('acct.assets')}
          hint={t('acct.assetsHint')}
          value={String(counts.assets)}
          icon={<Landmark className="size-4" />}
        />
        <MetricCard
          label={t('acct.income')}
          hint={t('acct.incomeHint')}
          value={String(counts.income)}
          icon={<TrendingUp className="size-4" />}
          accent="success"
        />
        <MetricCard
          label={t('acct.expenses')}
          hint={t('acct.expensesHint')}
          value={String(counts.expense)}
          icon={<TrendingDown className="size-4" />}
          accent="danger"
        />
      </div>

      {showForm ? (
        <Card>
          <div className="mb-5">
            <h3 className="text-base leading-6 font-semibold text-[var(--color-fg)]">{t('acct.newAccount')}</h3>
            <p className="text-[13px] leading-5 text-[var(--color-muted)]">{t('acct.newAccountHint')}</p>
          </div>
          <form noValidate onSubmit={onCreate} className="grid gap-4 sm:grid-cols-3">
            <Field label={t('acct.code')}>
              <Input
                ref={codeInputRef}
                value={code}
                onChange={(e) => {
                  setCode(e.target.value)
                  setFormError(null)
                }}
                placeholder={t('acct.codePlaceholder')}
                className="tabular-nums"
                required
              />
            </Field>
            <Field label={t('acct.name')}>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={t('acct.namePlaceholder')}
                required
              />
            </Field>
            <Field label={t('acct.type')}>
              <Select
                value={accountType}
                onChange={(e) => setAccountType(e.target.value as AccountType)}
              >
                {TYPES.map((opt) => (
                  <option key={opt.id} value={opt.id}>
                    {t(opt.labelKey)}
                  </option>
                ))}
              </Select>
            </Field>
            <div className="sm:col-span-3">
              <ErrorBanner message={formError} onDismiss={() => setFormError(null)} />
              <Button type="submit" busy={busy}>
                {busy ? t('common.saving') : t('acct.createAccount')}
              </Button>
            </div>
          </form>
        </Card>
      ) : null}

      <Modal
        open={balanceAccount !== null}
        title={
          balanceAccount
            ? t('acct.setBalanceNamed', { name: balanceAccount.name })
            : t('acct.setBalance')
        }
        description={owes ? t('accounts.balance.owedDescription') : t('acct.setBalanceDesc')}
        maxWidth="max-w-md"
        error={balanceError}
        onDismissError={() => setBalanceError(null)}
        onClose={() => {
          if (!balanceBusy) setBalanceAccount(null)
        }}
      >
        <form noValidate onSubmit={onSetBalance} className="space-y-4">
          {balanceCurrent !== null ? (
            <p className="text-sm text-[var(--color-muted)]">
              {owes ? t('accounts.balance.owedToday') : t('acct.ledgerBalanceToday')}{' '}
              <span className="font-medium tabular-nums text-[var(--color-fg)]">
                {formatMoney(balanceCurrent, bookCurrency(entity))}
              </span>
            </p>
          ) : null}
          {owes ? (
            <p className="text-[13px] leading-5 text-[var(--color-muted)]">
              {t('accounts.balance.owedHelp')}
            </p>
          ) : null}
          <Field
            label={
              owes
                ? t('accounts.balance.owedLabel', { ccy: entity.base_currency })
                : t('acct.actualBalance', { ccy: entity.base_currency })
            }
          >
            <Input
              inputMode="decimal"
              placeholder="2.500,00"
              value={balanceAmount}
              onChange={(e) => {
                setBalanceAmount(e.target.value)
                setBalanceError(null)
              }}
              className="tabular-nums"
              required
              aria-invalid={balanceError ? true : undefined}
            />
          </Field>
          {owes && enteredMinor !== null && enteredMinor < 0 ? (
            <p
              role="status"
              className="rounded-xl border border-[var(--color-warning)]/25 bg-[var(--color-warning-soft)] px-4 py-3 text-sm text-[var(--color-fg-secondary)]"
            >
              {t('accounts.balance.owedNegative')}
            </p>
          ) : null}
          <Field label={t('acct.asOf')}>
            <DateInput
              value={balanceAsOf}
              onChange={setBalanceAsOf}
              required
              aria-label={t('acct.balanceAsOf')}
            />
          </Field>
          <div className="flex justify-end gap-2 border-t border-[var(--color-border)] pt-4">
            <Button
              type="button"
              variant="secondary"
              disabled={balanceBusy}
              onClick={() => setBalanceAccount(null)}
            >
              {t('common.cancel')}
            </Button>
            <Button type="submit" busy={balanceBusy}>
              {balanceBusy ? t('acct.posting') : t('acct.setBalance')}
            </Button>
          </div>
        </form>
      </Modal>

      <ConfirmDialog
        open={deactivateTarget !== null}
        title={t('accounts.deactivate.confirmTitle', { name: deactivateTarget?.name ?? '' })}
        body={t('accounts.deactivate.confirmBody')}
        confirmLabel={t('acct.deactivate')}
        danger
        busy={deactivateBusy}
        onCancel={() => {
          if (!deactivateBusy) setDeactivateTarget(null)
        }}
        onConfirm={() => void confirmDeactivate()}
      />

      <RenameDialog
        current={renameTarget?.name ?? null}
        title={t('accounts.rename.title')}
        label={t('acct.name')}
        onSave={(name) => (renameTarget ? rename(renameTarget, name) : Promise.resolve())}
        onClose={() => setRenameTarget(null)}
      />

      {accounts.length === 0 ? (
        <EmptyState
          icon={<Banknote className="size-5" />}
          title={t('acct.noAccountsTitle')}
          body={t('acct.noAccountsBody')}
        />
      ) : (
        <Panel
          title={t('acct.chartTitle')}
          description={t('acct.chartDesc')}
          icon={<Banknote className="size-4" />}
        >
          <ul className="divide-y divide-[var(--color-border)]">
            {sortedAccounts.map((a) => {
              const meta = typeMeta(a.account_type)
              const Icon = meta.icon
              const balance = balances.get(a.id)
              return (
                // The whole row opens the register; the icon buttons stop the click.
                <li
                  key={a.id}
                  onClick={() => openRegister(a)}
                  className={cn(
                    'flex cursor-pointer items-center gap-4 px-5 py-3 transition hover:bg-[var(--color-surface-2)]/50',
                    !a.is_active && 'opacity-45',
                  )}
                >
                  <IconBadge tone={typeTone(a.account_type)}>
                    <Icon className="size-4" strokeWidth={1.75} />
                  </IconBadge>
                  <button type="button" className="min-w-0 flex-1 text-left">
                    <div className="flex flex-wrap items-baseline gap-x-2">
                      {/* A fixed code column, so names line up whatever the code's length. */}
                      <span className="inline-block min-w-10 text-sm font-medium tabular-nums text-[var(--color-fg)]">
                        {a.code}
                      </span>
                      <span className="text-sm text-[var(--color-fg)]">{a.name}</span>
                      {a.is_system ? (
                        <span className="text-xs text-[var(--color-muted)]">{t('common.system')}</span>
                      ) : null}
                    </div>
                    <div className="text-xs text-[var(--color-muted)]">
                      {t(meta.labelKey)}
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      {a.is_active ? t('common.active') : t('common.inactive')}
                    </div>
                  </button>
                  <span
                    className="min-w-28 text-right text-sm font-medium tabular-nums text-[var(--color-fg)]"
                    aria-label={balance === undefined ? undefined : t('accounts.list.balance')}
                  >
                    {balance === undefined ? '' : formatMoney(balance, bookCurrency(entity))}
                  </span>
                  <Button
                    variant="ghost"
                    size="iconSm"
                    onClick={(e) => {
                      e.stopPropagation()
                      openRegister(a)
                    }}
                    aria-label={t('accounts.register.title', { name: a.name })}
                  >
                    <Receipt className="size-4" />
                  </Button>
                  {a.is_active &&
                  (a.account_type === 'asset' || a.account_type === 'liability') ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="w-32 shrink-0"
                      onClick={(e) => {
                        e.stopPropagation()
                        openBalance(a)
                      }}
                      aria-label={t('acct.setBalanceAria', { name: a.name })}
                    >
                      <Coins className="size-4" />
                      {t('acct.setBalance')}
                    </Button>
                  ) : (
                    // Keeps the actions in columns when a row has fewer of them.
                    <span aria-hidden className="hidden w-32 shrink-0 sm:block" />
                  )}
                  <Button
                    variant="ghost"
                    size="iconSm"
                    onClick={(e) => {
                      e.stopPropagation()
                      setRenameTarget(a)
                    }}
                    aria-label={t('accounts.rename.aria', { name: a.name })}
                    title={t('common.rename')}
                  >
                    <PencilLine className="size-4" />
                  </Button>
                  {a.is_active && !a.is_system ? (
                    <Button
                      variant="ghost"
                      size="iconSm"
                      onClick={(e) => {
                        e.stopPropagation()
                        setDeactivateTarget(a)
                      }}
                      aria-label={t('acct.deactivateAria')}
                    >
                      <CircleOff className="size-4" />
                    </Button>
                  ) : !a.is_active ? (
                    <Button
                      variant="ghost"
                      size="iconSm"
                      onClick={(e) => {
                        e.stopPropagation()
                        void reactivate(a)
                      }}
                      aria-label={t('accounts.reactivate.aria', { name: a.name })}
                      title={t('accounts.reactivate.title')}
                    >
                      <RotateCcw className="size-4" />
                    </Button>
                  ) : (
                    <span aria-hidden className="size-8 shrink-0" />
                  )}
                </li>
              )
            })}
          </ul>
        </Panel>
      )}
    </div>
  )
}
