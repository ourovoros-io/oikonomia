import { useEffect, useMemo, useState, type FormEvent } from 'react'
import {
  Banknote,
  CircleOff,
  CreditCard,
  Landmark,
  PieChart,
  Plus,
  TrendingDown,
  TrendingUp,
  Wallet,
} from 'lucide-react'
import { api, type Account, type AccountType, type Entity } from '../lib/api'
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
  cn,
} from '../components/ui'
import type { CommandError } from '../lib/tauri'

type Props = { entity: Entity | null }

const TYPES: Array<{ id: AccountType; label: string; icon: typeof Wallet }> = [
  { id: 'asset', label: 'Asset', icon: Landmark },
  { id: 'liability', label: 'Liability', icon: CreditCard },
  { id: 'equity', label: 'Equity', icon: PieChart },
  { id: 'income', label: 'Income', icon: TrendingUp },
  { id: 'expense', label: 'Expense', icon: TrendingDown },
]

function typeMeta(t: AccountType) {
  return TYPES.find((x) => x.id === t) ?? TYPES[0]
}

function typeTone(t: AccountType): 'accent' | 'success' | 'danger' | 'muted' {
  if (t === 'income') return 'success'
  if (t === 'expense') return 'danger'
  if (t === 'asset') return 'accent'
  return 'muted'
}

export function AccountsPage({ entity }: Props) {
  const [accounts, setAccounts] = useState<Account[]>([])
  const [error, setError] = useState<string | null>(null)
  const [showForm, setShowForm] = useState(false)
  const [code, setCode] = useState('')
  const [name, setName] = useState('')
  const [accountType, setAccountType] = useState<AccountType>('expense')
  const [busy, setBusy] = useState(false)

  async function reload() {
    if (!entity) return
    setAccounts(await api.accountList(entity.id))
  }

  useEffect(() => {
    if (!entity) {
      setAccounts([])
      return
    }
    void reload().catch((err) => setError((err as CommandError).message))
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
      setError((err as CommandError).message)
    } finally {
      setBusy(false)
    }
  }

  async function onArchive(id: string) {
    try {
      await api.accountArchive(id)
      await reload()
    } catch (err) {
      setError((err as CommandError).message)
    }
  }

  if (!entity) {
    return (
      <EmptyState
        icon={<Wallet className="size-5" />}
        title="No book selected"
        body="Create or select a book first."
      />
    )
  }

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Chart"
        title="Accounts"
        description={`Chart of accounts for ${entity.name}`}
        meta={`${counts.total} active`}
        actions={
          <Button onClick={() => setShowForm((v) => !v)}>
            <Plus className="size-4" />
            {showForm ? 'Close' : 'Add account'}
          </Button>
        }
      />

      <ErrorBanner message={error} />

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <MetricCard
          label="Active accounts"
          hint="In this book"
          value={String(counts.total)}
          icon={<Wallet className="size-4" />}
        />
        <MetricCard
          label="Assets"
          hint="Cash, bank, inventory"
          value={String(counts.assets)}
          icon={<Landmark className="size-4" />}
        />
        <MetricCard
          label="Income"
          hint="Revenue categories"
          value={String(counts.income)}
          icon={<TrendingUp className="size-4" />}
          accent="success"
        />
        <MetricCard
          label="Expenses"
          hint="Spend categories"
          value={String(counts.expense)}
          icon={<TrendingDown className="size-4" />}
          accent="danger"
        />
      </div>

      {showForm ? (
        <Card padding="lg">
          <div className="mb-5">
            <h3 className="text-sm font-semibold text-[var(--color-fg)]">New account</h3>
            <p className="text-xs text-[var(--color-muted)]">
              Code, name, and type — system accounts cannot be removed
            </p>
          </div>
          <form onSubmit={onCreate} className="grid gap-4 sm:grid-cols-3">
            <Field label="Code">
              <Input
                value={code}
                onChange={(e) => setCode(e.target.value)}
                placeholder="e.g. 5150"
                className="tabular-nums"
                required
              />
            </Field>
            <Field label="Name">
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Account name"
                required
              />
            </Field>
            <Field label="Type">
              <Select
                value={accountType}
                onChange={(e) => setAccountType(e.target.value as AccountType)}
              >
                {TYPES.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.label}
                  </option>
                ))}
              </Select>
            </Field>
            <div className="flex items-end sm:col-span-3">
              <Button type="submit" busy={busy}>
                {busy ? 'Saving…' : 'Create account'}
              </Button>
            </div>
          </form>
        </Card>
      ) : null}

      {accounts.length === 0 ? (
        <EmptyState
          icon={<Banknote className="size-5" />}
          title="No accounts"
          body="This entity has an empty chart of accounts."
        />
      ) : (
        <Panel
          title="Chart of accounts"
          description="Active and archived accounts"
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
                        <span className="text-xs text-[var(--color-muted)]">system</span>
                      ) : null}
                    </div>
                    <div className="text-xs text-[var(--color-muted)]">
                      {meta.label}
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      {a.is_active ? 'Active' : 'Inactive'}
                    </div>
                  </div>
                  {a.is_active && !a.is_system ? (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 shrink-0"
                      onClick={() => void onArchive(a.id)}
                      aria-label="Deactivate account"
                      title="Deactivate"
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
