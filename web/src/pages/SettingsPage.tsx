import { useEffect, useState, type FormEvent } from 'react'
import {
  Briefcase,
  Building2,
  Clock,
  FileQuestion,
  Timer,
  Trash2,
  User,
} from 'lucide-react'
import { api, type ChartTemplate, type Entity } from '../lib/api'
import { CURRENCIES } from '../lib/currencies'
import { ConfirmDialog } from '../components/ConfirmDialog'
import {
  Button,
  Card,
  ChoiceCard,
  ErrorBanner,
  Field,
  IconBadge,
  Input,
  PageHeader,
  Panel,
  Select,
} from '../components/ui'
import type { CommandError } from '../lib/tauri'

type Props = {
  entities: Entity[]
  onEntitiesChange: () => Promise<void>
  onSelectEntity: (id: string) => void
}

const TEMPLATES: Array<{
  id: ChartTemplate
  title: string
  description: string
  icon: typeof User
}> = [
  {
    id: 'personal',
    title: 'Personal',
    description: 'Cash, cards, salary, living costs',
    icon: User,
  },
  {
    id: 'company',
    title: 'Company',
    description: 'AR/AP, sales, payroll, opex',
    icon: Briefcase,
  },
  {
    id: 'blank',
    title: 'Blank',
    description: 'Start with an empty chart',
    icon: FileQuestion,
  },
]

const LOCK_PRESETS = [
  { mins: 5, label: '5 min' },
  { mins: 15, label: '15 min' },
  { mins: 30, label: '30 min' },
  { mins: 60, label: '1 hour' },
]

export function SettingsPage({ entities, onEntitiesChange, onSelectEntity }: Props) {
  const [error, setError] = useState<string | null>(null)
  const [name, setName] = useState('')
  const [currency, setCurrency] = useState('EUR')
  const [template, setTemplate] = useState<ChartTemplate>('personal')
  const [busy, setBusy] = useState(false)
  const [lockMins, setLockMins] = useState(15)
  const [lockBusy, setLockBusy] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<{ id: string; name: string } | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)

  useEffect(() => {
    void api
      .getLockTimeout()
      .then((secs) => setLockMins(Math.max(1, Math.round(secs / 60))))
      .catch(() => {
        /* ignore */
      })
  }, [])

  async function onCreate(ev: FormEvent) {
    ev.preventDefault()
    setBusy(true)
    setError(null)
    try {
      const entity = await api.entityCreate({
        name,
        base_currency: currency,
        chart_template: template,
        fiscal_year_start_month: 1,
      })
      setName('')
      await onEntitiesChange()
      onSelectEntity(entity.id)
    } catch (err) {
      setError((err as CommandError).message)
    } finally {
      setBusy(false)
    }
  }

  async function confirmDelete() {
    if (!pendingDelete) return
    setDeleteBusy(true)
    setError(null)
    try {
      await api.entityDelete(pendingDelete.id)
      setPendingDelete(null)
      await onEntitiesChange()
    } catch (err) {
      setError((err as CommandError).message || 'Failed to delete entity')
    } finally {
      setDeleteBusy(false)
    }
  }

  async function saveLock(mins: number) {
    if (!Number.isFinite(mins) || mins < 1) {
      setError('Lock timeout must be at least 1 minute')
      return
    }
    setLockBusy(true)
    setError(null)
    try {
      await api.setLockTimeout(Math.round(mins * 60))
      setLockMins(mins)
    } catch (err) {
      setError((err as CommandError).message)
    } finally {
      setLockBusy(false)
    }
  }

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Workspace"
        title="Settings"
        description="Books, security, and preferences"
        meta="Local vault only"
      />

      <ErrorBanner message={error} />

      <ConfirmDialog
        open={pendingDelete !== null}
        title="Delete entity?"
        body={
          pendingDelete
            ? `“${pendingDelete.name}” and all of its accounts and transactions will be permanently removed. This cannot be undone.`
            : ''
        }
        confirmLabel="Delete"
        danger
        busy={deleteBusy}
        onCancel={() => {
          if (!deleteBusy) setPendingDelete(null)
        }}
        onConfirm={() => void confirmDelete()}
      />

      <Card padding="lg">
        <div className="mb-5 flex items-center gap-3">
          <IconBadge tone="accent" size="sm">
            <Timer className="size-4" />
          </IconBadge>
          <div>
            <h3 className="text-sm font-semibold text-[var(--color-fg)]">Auto-lock</h3>
            <p className="text-xs text-[var(--color-muted)]">
              Lock the vault after idle time
            </p>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {LOCK_PRESETS.map((p) => (
            <Button
              key={p.mins}
              variant={lockMins === p.mins ? 'primary' : 'secondary'}
              size="sm"
              disabled={lockBusy}
              onClick={() => void saveLock(p.mins)}
            >
              <Clock className="size-3.5" />
              {p.label}
            </Button>
          ))}
        </div>
      </Card>

      <Card padding="lg">
        <div className="mb-5 flex items-center gap-3">
          <IconBadge tone="accent" size="sm">
            <Building2 className="size-4" />
          </IconBadge>
          <div>
            <h3 className="text-sm font-semibold text-[var(--color-fg)]">New entity</h3>
            <p className="text-xs text-[var(--color-muted)]">
              Separate books for personal and company
            </p>
          </div>
        </div>

        <form onSubmit={onCreate} className="space-y-4">
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Name">
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                required
                placeholder="Personal"
              />
            </Field>
            <Field label="Currency">
              <Select value={currency} onChange={(e) => setCurrency(e.target.value)} required>
                {CURRENCIES.map((c) => (
                  <option key={c.code} value={c.code}>
                    {c.code} — {c.label}
                  </option>
                ))}
              </Select>
            </Field>
          </div>

          <div>
            <span className="mb-1.5 block text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
              Chart template
            </span>
            <div className="grid gap-3 sm:grid-cols-3">
              {TEMPLATES.map((t) => {
                const Icon = t.icon
                return (
                  <ChoiceCard
                    key={t.id}
                    selected={template === t.id}
                    onClick={() => setTemplate(t.id)}
                    icon={<Icon className="size-4" strokeWidth={1.75} />}
                    title={t.title}
                    description={t.description}
                  />
                )
              })}
            </div>
          </div>

          <Button type="submit" disabled={busy}>
            {busy ? 'Creating…' : 'Create entity'}
          </Button>
        </form>
      </Card>

      <Panel
        title="Entities"
        description={
          entities.length === 0
            ? 'No books yet'
            : `${entities.length} book${entities.length === 1 ? '' : 's'}`
        }
        icon={<Building2 className="size-4" />}
      >
        {entities.length === 0 ? (
          <div className="px-5 py-12 text-center text-sm text-[var(--color-muted)]">
            Create an entity above to start posting entries.
          </div>
        ) : (
          <ul className="divide-y divide-[var(--color-border)]">
            {entities.map((e) => (
              <li
                key={e.id}
                className="flex items-center gap-4 px-5 py-3.5 transition hover:bg-[var(--color-surface-2)]/50"
              >
                <IconBadge tone="accent">
                  <Building2 className="size-4" />
                </IconBadge>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium text-[var(--color-fg)]">
                    {e.name}
                  </div>
                  <div className="text-xs text-[var(--color-muted)]">
                    <span className="tabular-nums">{e.base_currency}</span>
                    <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                    <span className="capitalize">{e.chart_template}</span>
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <Button variant="secondary" size="sm" onClick={() => onSelectEntity(e.id)}>
                    Open
                  </Button>
                  <Button
                    variant="danger"
                    size="icon"
                    className="h-8 w-8"
                    onClick={() => setPendingDelete({ id: e.id, name: e.name })}
                    aria-label={`Delete ${e.name}`}
                    title="Delete"
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Panel>
    </div>
  )
}
