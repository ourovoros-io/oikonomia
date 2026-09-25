import { useEffect, useMemo, useState, type FormEvent } from 'react'
import {
  Banknote,
  CircleOff,
  Coins,
  CreditCard,
  Landmark,
  PieChart,
  Plus,
  Receipt,
  TrendingDown,
  TrendingUp,
  Wallet,
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
import { parseMajorToMinor } from '../lib/money'
import { DateInput } from '../components/DateInput'
import { HiddenBadge } from '../components/hiddenUi'
import { Modal } from '../components/Modal'
import {
  Button,
  Card,
  EmptyState,
  ErrorBanner,
  Field,
  IconBadge,
  Input,
  MetricCard,
  PageHeader,
  Panel,
  Select,
} from '../components/ui'
import { cn } from '../lib/cn'
import { commandErrorMessage } from '../lib/commandError'
import type { CommandError } from '../lib/tauri'
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

export function AccountsPage({ entity, onCreateBook }: Props) {
  const { t } = useI18n()
  const [accounts, setAccounts] = useState<Account[]>([])
  const [error, setError] = useState<string | null>(null)
  const [showForm, setShowForm] = useState(false)
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
    setAccounts(await api.accountList(entity.id))
  }

  useEffect(() => {
    if (!entity) {
      setAccounts([])
      return
    }
    void reload().catch((err) => setError(commandErrorMessage(err as CommandError)))
  }, [entity?.id])

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
    setError(null)
    try {
      await api.accountCreate({
        entity_id: entity.id,
        code,
        name,
        account_type: accountType,
      })
      setCode('')
      setName('')
      setShowForm(false)
      await reload()
    } catch (err) {
      setError(commandErrorMessage(err as CommandError))
    } finally {
      setBusy(false)
    }
  }

  async function onArchive(id: string) {
    try {
      await api.accountArchive(id)
      await reload()
    } catch (err) {
      setError(commandErrorMessage(err as CommandError))
    }
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
        if (!cancelled) setRegisterError(commandErrorMessage(err as CommandError))
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

    const minor = parseMajorToMinor(balanceAmount, entity.base_currency)
    if (minor === null) {
      setBalanceError(t('acct.invalidAmount'))
      return
    }

    setBalanceBusy(true)
    setBalanceError(null)
    try {
      await api.accountSetOpeningBalance(balanceAccount.id, minor, balanceAsOf)
      setBalanceAccount(null)
      await reload()
    } catch (err) {
      setBalanceError(commandErrorMessage(err as CommandError))
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
      <div className="space-y-6">
        <PageHeader
          eyebrow={t('acct.eyebrow')}
          title={t('accounts.register.title', { name: registerAccount.name })}
          actions={
            <Button variant="secondary" onClick={() => setRegisterAccount(null)}>
              {t('accounts.register.back')}
            </Button>
          }
        />

        <ErrorBanner message={registerError} />

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
                      <td className="px-5 py-3 whitespace-nowrap text-xs text-[var(--color-muted)]">
                        {formatDate(line.entry_date)}
                      </td>
                      <td className="px-5 py-3">
                        <span className="text-[var(--color-fg)]">{line.description}</span>
                        {line.hidden ? <HiddenBadge className="ml-2" /> : null}
                      </td>
                      <td className="px-5 py-3 text-right tabular-nums text-[var(--color-fg)]">
                        {line.debit_minor ? formatMoney(line.debit_minor, entity.base_currency) : ''}
                      </td>
                      <td className="px-5 py-3 text-right tabular-nums text-[var(--color-fg)]">
                        {line.credit_minor ? formatMoney(line.credit_minor, entity.base_currency) : ''}
                      </td>
                      <td className="px-5 py-3 text-right font-medium tabular-nums text-[var(--color-fg)]">
                        {formatMoney(line.balance_minor, entity.base_currency)}
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

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow={t('acct.eyebrow')}
        title={t('acct.title')}
        description={t('acct.description', { name: entity.name })}
        meta={t('acct.meta', { count: counts.total })}
        actions={
          <Button onClick={() => setShowForm((v) => !v)}>
            <Plus className="size-4" />
            {showForm ? t('acct.close') : t('acct.addAccount')}
          </Button>
        }
      />

      <ErrorBanner message={error} />

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
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
        <Card padding="lg">
          <div className="mb-5">
            <h3 className="text-sm font-semibold text-[var(--color-fg)]">{t('acct.newAccount')}</h3>
            <p className="text-xs text-[var(--color-muted)]">{t('acct.newAccountHint')}</p>
          </div>
          <form onSubmit={onCreate} className="grid gap-4 sm:grid-cols-3">
            <Field label={t('acct.code')}>
              <Input
                value={code}
                onChange={(e) => setCode(e.target.value)}
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
            <div className="flex items-end sm:col-span-3">
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
        description={t('acct.setBalanceDesc')}
        maxWidth="max-w-md"
        onClose={() => {
          if (!balanceBusy) setBalanceAccount(null)
        }}
      >
        <form onSubmit={onSetBalance} className="space-y-4">
          <ErrorBanner message={balanceError} className="mb-0" />
          {balanceCurrent !== null ? (
            <p className="text-sm text-[var(--color-muted)]">
              {t('acct.ledgerBalanceToday')}{' '}
              <span className="font-medium tabular-nums text-[var(--color-fg)]">
                {formatMoney(balanceCurrent, entity.base_currency)}
              </span>
            </p>
          ) : null}
          <Field label={t('acct.actualBalance', { ccy: entity.base_currency })}>
            <Input
              inputMode="decimal"
              placeholder="2.500,00"
              value={balanceAmount}
              onChange={(e) => setBalanceAmount(e.target.value)}
              className="tabular-nums"
              required
              autoFocus
            />
          </Field>
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
            {accounts.map((a) => {
              const meta = typeMeta(a.account_type)
              const Icon = meta.icon
              return (
                <li
                  key={a.id}
                  className={cn(
                    'flex items-center gap-4 px-5 py-3.5 transition hover:bg-[var(--color-surface-2)]/50',
                    !a.is_active && 'opacity-45',
                  )}
                >
                  <IconBadge tone={typeTone(a.account_type)}>
                    <Icon className="size-4" strokeWidth={1.75} />
                  </IconBadge>
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-baseline gap-x-2">
                      <span className="text-sm font-medium tabular-nums text-[var(--color-fg)]">
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
                  </div>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    onClick={() => openRegister(a)}
                    aria-label={t('accounts.register.title', { name: a.name })}
                    title={t('accounts.register.title', { name: a.name })}
                  >
                    <Receipt className="size-4" />
                  </Button>
                  {a.is_active &&
                  (a.account_type === 'asset' || a.account_type === 'liability') ? (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 shrink-0"
                      onClick={() => openBalance(a)}
                      aria-label={t('acct.setBalanceAria', { name: a.name })}
                      title={t('acct.setBalance')}
                    >
                      <Coins className="size-4" />
                    </Button>
                  ) : null}
                  {a.is_active && !a.is_system ? (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 shrink-0"
                      onClick={() => void onArchive(a.id)}
                      aria-label={t('acct.deactivateAria')}
                      title={t('acct.deactivate')}
                    >
                      <CircleOff className="size-4" />
                    </Button>
                  ) : (
                    <span className="inline-block h-8 w-8 shrink-0" />
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
