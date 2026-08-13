import { useEffect, useState, type FormEvent } from 'react'
import {
  Archive,
  Briefcase,
  Building2,
  Clock,
  Download,
  FileQuestion,
  KeyRound,
  Plus,
  Timer,
  Trash2,
  Upload,
  User,
} from 'lucide-react'
import { api, type ChartTemplate, type Entity } from '../lib/api'
import { vaultBackup, vaultChangePassword, vaultRestore, pickVaultBackup, type CommandError } from '../lib/tauri'
import { CURRENCIES } from '../lib/currencies'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { Modal } from '../components/Modal'
import {
  Button,
  ChoiceCard,
  CollapsibleSection,
  ErrorBanner,
  Field,
  IconBadge,
  Input,
  PageHeader,
  Select,
} from '../components/ui'
import {
  VAULT_BACKUP_BODY,
  VAULT_BACKUP_HINT,
  backupCommandError,
  canBackupVault,
  restoreArgs,
  restoreConfirm,
  vaultBackupAvailability,
  vaultBackupBanner,
} from '../lib/vaultBackupUi'

type Props = {
  entities: Entity[]
  /** False when no vault files exist. Settings is normally only mounted unlocked. */
  vaultPresent?: boolean
  onEntitiesChange: () => Promise<void>
  onSelectEntity: (id: string) => void
  onLockTimeoutChange?: (secs: number) => void
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

export function SettingsPage({
  entities,
  vaultPresent = true,
  onEntitiesChange,
  onSelectEntity,
  onLockTimeoutChange,
}: Props) {
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [name, setName] = useState('')
  const [currency, setCurrency] = useState('EUR')
  const [template, setTemplate] = useState<ChartTemplate>('personal')
  const [busy, setBusy] = useState(false)
  const [showCreate, setShowCreate] = useState(false)
  const [lockMins, setLockMins] = useState(15)
  const [lockBusy, setLockBusy] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<{ id: string; name: string } | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const [oldPassword, setOldPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [passwordBusy, setPasswordBusy] = useState(false)
  const [backupBusy, setBackupBusy] = useState(false)
  const [restoreOpen, setRestoreOpen] = useState(false)
  const [restoreBusy, setRestoreBusy] = useState(false)
  const [restorePath, setRestorePath] = useState<string | undefined>(undefined)
  const [restorePicking, setRestorePicking] = useState(false)

  const backupAvailability = vaultBackupAvailability({
    vaultPresent,
    entityCount: entities.length,
  })
  const backupBanner = vaultBackupBanner(backupAvailability)
  const backupEnabled = canBackupVault(backupAvailability)
  const replaceConfirm = restoreConfirm('replace')

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
      setShowCreate(false)
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
      const secs = Math.round(mins * 60)
      await api.setLockTimeout(secs)
      setLockMins(mins)
      onLockTimeoutChange?.(secs)
    } catch (err) {
      setError((err as CommandError).message)
    } finally {
      setLockBusy(false)
    }
  }

  async function onChangePassword(ev: FormEvent) {
    ev.preventDefault()
    setError(null)
    setNotice(null)

    if (newPassword !== confirmPassword) {
      setError('New passwords do not match')
      return
    }
    // Password strength rules live in Rust; its Validation error surfaces below.

    setPasswordBusy(true)
    try {
      await vaultChangePassword(oldPassword, newPassword)
      setOldPassword('')
      setNewPassword('')
      setConfirmPassword('')
      setNotice('Password changed. The vault is re-encrypted under the new password.')
    } catch (err) {
      const cmd = err as CommandError
      setError(
        cmd.code === 'invalid_password'
          ? 'Current password is incorrect.'
          : cmd.message || 'Could not change the password',
      )
    } finally {
      setPasswordBusy(false)
    }
  }

  async function onBackup() {
    if (!backupEnabled) return
    setError(null)
    setNotice(null)
    setBackupBusy(true)
    try {
      await vaultBackup()
    } catch (err) {
      setError(backupCommandError(err as CommandError))
    } finally {
      setBackupBusy(false)
    }
  }

  async function beginRestore() {
    if (restoreBusy || restorePicking || restoreOpen) return
    setError(null)
    setRestorePicking(true)
    try {
      const picked = await pickVaultBackup()
      if (picked.kind === 'cancelled') return
      setRestorePath(picked.kind === 'picked' ? picked.path : undefined)
      setRestoreOpen(true)
    } catch (err) {
      setError(backupCommandError(err as CommandError))
    } finally {
      setRestorePicking(false)
    }
  }

  async function confirmRestore() {
    setRestoreBusy(true)
    setError(null)
    try {
      const result = await vaultRestore(restoreArgs(restorePath, replaceConfirm.replace))
      if (result === null) {
        setRestoreOpen(false)
        setRestorePath(undefined)
        return
      }
      setRestoreOpen(false)
      setRestorePath(undefined)
    } catch (err) {
      setError(backupCommandError(err as CommandError))
      setRestoreOpen(false)
      setRestorePath(undefined)
    } finally {
      setRestoreBusy(false)
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
      {notice ? (
        <div className="rounded-xl border border-[var(--color-accent)]/25 bg-[var(--color-accent-soft)] px-4 py-3 text-sm text-[var(--color-fg-secondary)]">
          {notice}
        </div>
      ) : null}

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

      <ConfirmDialog
        open={restoreOpen}
        title={replaceConfirm.title}
        body={replaceConfirm.body}
        confirmLabel={replaceConfirm.confirmLabel}
        danger
        busy={restoreBusy}
        onCancel={() => {
          if (!restoreBusy) {
            setRestoreOpen(false)
            setRestorePath(undefined)
          }
        }}
        onConfirm={() => void confirmRestore()}
      />

      <CollapsibleSection
        title="Auto-lock"
        description="Lock the vault after idle time"
        icon={<Timer className="size-4" />}
        tone="warning"
      >
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
      </CollapsibleSection>

      <CollapsibleSection
        title="Master password"
        description="Re-encrypts the vault. There is no recovery if the new password is lost."
        icon={<KeyRound className="size-4" />}
      >
        <form onSubmit={onChangePassword} className="grid gap-4 sm:grid-cols-3">
          <Field label="Current password">
            <Input
              type="password"
              autoComplete="current-password"
              value={oldPassword}
              onChange={(e) => setOldPassword(e.target.value)}
              required
            />
          </Field>
          <Field label="New password">
            <Input
              type="password"
              autoComplete="new-password"
              value={newPassword}
              onChange={(e) => setNewPassword(e.target.value)}
              required
            />
          </Field>
          <Field label="Confirm new password">
            <Input
              type="password"
              autoComplete="new-password"
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              required
            />
          </Field>
          <div className="sm:col-span-3">
            <Button type="submit" busy={passwordBusy}>
              {passwordBusy ? 'Re-encrypting…' : 'Change password'}
            </Button>
          </div>
        </form>
      </CollapsibleSection>

      <CollapsibleSection
        title="Vault backup"
        description="Export the encrypted vault as one file"
        icon={<Archive className="size-4" />}
        tone="accent"
      >
        {backupBanner ? (
          <ErrorBanner
            title={backupBanner.title}
            message={backupBanner.body}
            className="mb-4"
          />
        ) : null}
        <p className="text-sm leading-relaxed text-[var(--color-fg-secondary)]">
          {VAULT_BACKUP_BODY}
        </p>
        <p className="mt-2 text-xs text-[var(--color-muted)]">{VAULT_BACKUP_HINT}</p>
        <div className="mt-5 flex flex-wrap items-center gap-2">
          <Button
            disabled={!backupEnabled}
            busy={backupBusy}
            onClick={() => void onBackup()}
          >
            <Download className="size-3.5" />
            Backup vault
          </Button>
          <Button
            variant="danger"
            disabled={restoreBusy || restorePicking}
            busy={restorePicking}
            onClick={() => void beginRestore()}
          >
            <Upload className="size-3.5" />
            Restore from backup
          </Button>
        </div>
      </CollapsibleSection>

      <Modal
        open={showCreate}
        title="New entity"
        description="Separate books for personal and company"
        onClose={() => {
          if (!busy) setShowCreate(false)
        }}
      >
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

          <div className="flex justify-end gap-2 border-t border-[var(--color-border)] pt-4">
            <Button
              type="button"
              variant="secondary"
              disabled={busy}
              onClick={() => setShowCreate(false)}
            >
              Cancel
            </Button>
            <Button type="submit" busy={busy}>
              {busy ? 'Creating…' : 'Create entity'}
            </Button>
          </div>
        </form>
      </Modal>

      <CollapsibleSection
        title="Entities"
        description={
          entities.length === 0
            ? 'No books yet'
            : `${entities.length} book${entities.length === 1 ? '' : 's'}`
        }
        icon={<Building2 className="size-4" />}
        tone="success"
        flush
        actions={
          <Button size="sm" onClick={() => setShowCreate(true)}>
            <Plus className="size-3.5" />
            New entity
          </Button>
        }
      >
        {entities.length === 0 ? (
          <div className="px-5 py-12 text-center text-sm text-[var(--color-muted)]">
            Use the New entity button above to create your first book.
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
      </CollapsibleSection>
    </div>
  )
}
