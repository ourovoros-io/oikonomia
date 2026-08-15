import { useEffect, useState, type FormEvent } from 'react'
import {
  Archive,
  Briefcase,
  Building2,
  Clock,
  Download,
  FileQuestion,
  KeyRound,
  Languages,
  Plus,
  Timer,
  Trash2,
  Upload,
  User,
} from 'lucide-react'
import { api, type ChartTemplate, type Entity } from '../lib/api'
import { vaultBackup, vaultChangePassword, vaultPickBackup, vaultRestore, type CommandError } from '../lib/tauri'
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
  backupCommandError,
  canBackupVault,
  restoreConfirm,
  vaultBackupAvailability,
  vaultBackupBanner,
  vaultBackupBody,
  vaultBackupHint,
} from '../lib/vaultBackupUi'
import { useI18n } from '../lib/I18nProvider'
import { cn } from '../lib/cn'
import type { Locale } from '../lib/api'

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
  titleKey:
    | 'settings.entityCreate.template.personal.title'
    | 'settings.entityCreate.template.company.title'
    | 'settings.entityCreate.template.blank.title'
  descriptionKey:
    | 'settings.template.personal.description'
    | 'settings.template.company.description'
    | 'settings.template.blank.description'
  icon: typeof User
}> = [
  {
    id: 'personal',
    titleKey: 'settings.entityCreate.template.personal.title',
    descriptionKey: 'settings.template.personal.description',
    icon: User,
  },
  {
    id: 'company',
    titleKey: 'settings.entityCreate.template.company.title',
    descriptionKey: 'settings.template.company.description',
    icon: Briefcase,
  },
  {
    id: 'blank',
    titleKey: 'settings.entityCreate.template.blank.title',
    descriptionKey: 'settings.template.blank.description',
    icon: FileQuestion,
  },
]

/** Designer-locked language pill. Option labels stay native-script in both locales. */
function LanguagePill({
  value,
  onChange,
  ariaLabel,
  englishLabel,
  greekLabel,
}: {
  value: Locale
  onChange: (locale: Locale) => void
  ariaLabel: string
  englishLabel: string
  greekLabel: string
}) {
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className="inline-flex h-8 items-center rounded-full border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] p-[3px]"
    >
      {(
        [
          { id: 'en', label: englishLabel },
          { id: 'el', label: greekLabel },
        ] as const
      ).map((opt) => {
        const active = value === opt.id
        return (
          <button
            key={opt.id}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => onChange(opt.id)}
            className={cn(
              'inline-flex h-[26px] items-center rounded-full px-3 text-sm font-medium transition',
              active
                ? 'bg-[#f4f6f4] text-[#131b15] shadow-sm'
                : 'text-[var(--color-muted)]',
            )}
          >
            {opt.label}
          </button>
        )
      })}
    </div>
  )
}

const LOCK_PRESETS = [
  { mins: 5, labelKey: 'settings.lock.5min' },
  { mins: 15, labelKey: 'settings.lock.15min' },
  { mins: 30, labelKey: 'settings.lock.30min' },
  { mins: 60, labelKey: 'settings.lock.1hour' },
] as const

export function SettingsPage({
  entities,
  vaultPresent = true,
  onEntitiesChange,
  onSelectEntity,
  onLockTimeoutChange,
}: Props) {
  const { t, locale, setLocale } = useI18n()
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
      setError((err as CommandError).message || t('settings.deleteFailed'))
    } finally {
      setDeleteBusy(false)
    }
  }

  async function saveLock(mins: number) {
    if (!Number.isFinite(mins) || mins < 1) {
      setError(t('settings.lockTimeoutMin'))
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
      setError(t('settings.passwordsMismatch'))
      return
    }
    // Password strength rules live in Rust; its Validation error surfaces below.

    setPasswordBusy(true)
    try {
      await vaultChangePassword(oldPassword, newPassword)
      setOldPassword('')
      setNewPassword('')
      setConfirmPassword('')
      setNotice(t('settings.passwordChanged'))
    } catch (err) {
      const cmd = err as CommandError
      setError(
        cmd.code === 'invalid_password'
          ? t('settings.currentPasswordIncorrect')
          : cmd.message || t('settings.changePasswordFailed'),
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
      const path = await vaultPickBackup()
      if (path === null) return
      setRestorePath(path)
      setRestoreOpen(true)
    } catch (err) {
      setError(backupCommandError(err as CommandError))
    } finally {
      setRestorePicking(false)
    }
  }

  async function confirmRestore() {
    if (!restorePath) return
    setRestoreBusy(true)
    setError(null)
    try {
      const result = await vaultRestore({ path: restorePath, replace: replaceConfirm.replace })
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
        eyebrow={t('settings.eyebrow')}
        title={t('settings.title')}
        description={t('settings.description')}
        meta={t('settings.meta')}
      />

      <ErrorBanner message={error} />
      {notice ? (
        <div className="rounded-xl border border-[var(--color-accent)]/25 bg-[var(--color-accent-soft)] px-4 py-3 text-sm text-[var(--color-fg-secondary)]">
          {notice}
        </div>
      ) : null}

      <ConfirmDialog
        open={pendingDelete !== null}
        title={t('settings.deleteEntityTitle')}
        body={
          pendingDelete ? t('settings.deleteEntityBody', { name: pendingDelete.name }) : ''
        }
        confirmLabel={t('common.delete')}
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
        title={t('settings.language.title')}
        description={t('settings.language.description')}
        icon={<Languages className="size-4" />}
        tone="muted"
        defaultOpen
      >
        <LanguagePill
          value={locale}
          onChange={setLocale}
          ariaLabel={t('settings.language.title')}
          englishLabel={t('settings.language.english')}
          greekLabel={t('settings.language.greek')}
        />
      </CollapsibleSection>

      <CollapsibleSection
        title={t('settings.autoLock.title')}
        description={t('settings.autoLock.description')}
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
              {t(p.labelKey)}
            </Button>
          ))}
        </div>
      </CollapsibleSection>

      <CollapsibleSection
        title={t('settings.masterPassword.title')}
        description={t('settings.masterPassword.description')}
        icon={<KeyRound className="size-4" />}
      >
        <form onSubmit={onChangePassword} className="grid gap-4 sm:grid-cols-3">
          <Field label={t('settings.currentPassword')}>
            <Input
              type="password"
              autoComplete="current-password"
              value={oldPassword}
              onChange={(e) => setOldPassword(e.target.value)}
              required
            />
          </Field>
          <Field label={t('settings.newPassword')}>
            <Input
              type="password"
              autoComplete="new-password"
              value={newPassword}
              onChange={(e) => setNewPassword(e.target.value)}
              required
            />
          </Field>
          <Field label={t('settings.confirmNewPassword')}>
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
              {passwordBusy ? t('settings.reencrypting') : t('settings.changePassword')}
            </Button>
          </div>
        </form>
      </CollapsibleSection>

      <CollapsibleSection
        title={t('settings.vaultBackup.title')}
        description={t('settings.vaultBackup.description')}
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
          {vaultBackupBody()}
        </p>
        <p className="mt-2 text-xs text-[var(--color-muted)]">{vaultBackupHint()}</p>
        <div className="mt-5 flex flex-wrap items-center gap-2">
          <Button
            disabled={!backupEnabled}
            busy={backupBusy}
            onClick={() => void onBackup()}
          >
            <Download className="size-3.5" />
            {t('settings.vaultBackup.backupVault')}
          </Button>
          <Button
            variant="danger"
            disabled={restoreBusy || restorePicking}
            busy={restorePicking}
            onClick={() => void beginRestore()}
          >
            <Upload className="size-3.5" />
            {t('settings.vaultBackup.restore')}
          </Button>
        </div>
      </CollapsibleSection>

      <Modal
        open={showCreate}
        title={t('settings.newEntity.title')}
        description={t('settings.newEntity.description')}
        onClose={() => {
          if (!busy) setShowCreate(false)
        }}
      >
        <form onSubmit={onCreate} className="space-y-4">
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={t('settings.newEntity.name')}>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                required
                placeholder={t('settings.newEntity.namePlaceholder')}
              />
            </Field>
            <Field label={t('settings.newEntity.currency')}>
              <Select value={currency} onChange={(e) => setCurrency(e.target.value)} required>
                {CURRENCIES.map((c) => (
                  <option key={c.code} value={c.code}>
                    {c.code} — {t(`currency.${c.code}`)}
                  </option>
                ))}
              </Select>
            </Field>
          </div>

          <div>
            <span className="mb-1.5 block text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
              {t('settings.newEntity.chartTemplate')}
            </span>
            <div className="grid gap-3 sm:grid-cols-3">
              {TEMPLATES.map((tpl) => {
                const Icon = tpl.icon
                return (
                  <ChoiceCard
                    key={tpl.id}
                    selected={template === tpl.id}
                    onClick={() => setTemplate(tpl.id)}
                    icon={<Icon className="size-4" strokeWidth={1.75} />}
                    title={t(tpl.titleKey)}
                    description={t(tpl.descriptionKey)}
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
              {t('common.cancel')}
            </Button>
            <Button type="submit" busy={busy}>
              {busy ? t('settings.newEntity.creating') : t('settings.newEntity.create')}
            </Button>
          </div>
        </form>
      </Modal>

      <CollapsibleSection
        title={t('settings.entities.title')}
        description={
          entities.length === 0
            ? t('settings.entities.none')
            : entities.length === 1
              ? t('settings.entities.oneBook')
              : t('settings.entities.nBooks', { count: entities.length })
        }
        icon={<Building2 className="size-4" />}
        tone="success"
        flush
        actions={
          <Button size="sm" onClick={() => setShowCreate(true)}>
            <Plus className="size-3.5" />
            {t('settings.entities.new')}
          </Button>
        }
      >
        {entities.length === 0 ? (
          <div className="px-5 py-12 text-center text-sm text-[var(--color-muted)]">
            {t('settings.entities.empty')}
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
                    <span>{t(`chart.${e.chart_template}`)}</span>
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <Button variant="secondary" size="sm" onClick={() => onSelectEntity(e.id)}>
                    {t('settings.entities.open')}
                  </Button>
                  <Button
                    variant="danger"
                    size="icon"
                    className="h-8 w-8"
                    onClick={() => setPendingDelete({ id: e.id, name: e.name })}
                    aria-label={t('settings.entities.deleteAria', { name: e.name })}
                    title={t('common.delete')}
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
