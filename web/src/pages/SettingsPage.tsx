import { useEffect, useId, useRef, useState, type FormEvent } from 'react'
import {
  Archive,
  Briefcase,
  Building2,
  Clock,
  Download,
  FileQuestion,
  Key,
  KeyRound,
  Languages,
  LifeBuoy,
  Mail,
  Plus,
  Timer,
  Trash2,
  Upload,
  User,
} from 'lucide-react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { api, type ChartTemplate, type Entity } from '../lib/api'
import {
  canAddAnotherBook,
  formatLicensedUntil,
  isLicenseExpiredCode,
  licenseErrorMessage,
  licenseExpiredBanner,
  licenseImportError,
  type LicenseStatus,
} from '../lib/license'
import {
  vaultBackup,
  vaultChangePassword,
  vaultPickBackup,
  vaultRestore,
  type AppInfo,
  type CommandError,
} from '../lib/tauri'
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
import { LOCALES } from '../lib/i18n'
import { cn } from '../lib/cn'
import type { Locale } from '../lib/api'

type Props = {
  entities: Entity[]
  /** False when no vault files exist. Settings is normally only mounted unlocked. */
  vaultPresent?: boolean
  onEntitiesChange: () => Promise<void>
  onSelectEntity: (id: string) => void
  onLockTimeoutChange?: (secs: number) => void
  /** Bumped by App's empty-state CTAs to pop the new-entity form open and scroll to it. */
  createBookIntent?: number
  /** Called once the current createBookIntent has been consumed (form opened, scrolled to). */
  onCreateBookIntentHandled?: () => void
  /** Mirrors this page's license status up to App, so TrialBanner reflects an install/expiry without waiting for a relock or reload. */
  onLicenseChanged?: (status: LicenseStatus | null) => void
  /** Build identity from Rust `app_info`; the support address rides on it. Null until App has fetched it. */
  appInfo?: AppInfo | null
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

/** Language pills: one row of equal-width segments (fr columns under w-fit size to the widest autonym). */
function LanguagePill({
  value,
  onChange,
  ariaLabel,
}: {
  value: Locale
  onChange: (locale: Locale) => void
  ariaLabel: string
}) {
  const { t } = useI18n()
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className="grid w-fit grid-flow-col auto-cols-fr rounded-full border border-[var(--color-border-strong)] bg-[var(--color-surface-2)] p-[3px]"
    >
      {LOCALES.map((id) => {
        const active = value === id
        return (
          <button
            key={id}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => onChange(id)}
            className={cn(
              'inline-flex h-[26px] w-full items-center justify-center rounded-full px-3 text-sm font-medium transition',
              active
                ? 'bg-[#f4f6f4] text-[#131b15] shadow-sm'
                : 'text-[var(--color-muted)]',
            )}
          >
            {t(`settings.language.option.${id}`)}
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
  createBookIntent,
  onCreateBookIntentHandled,
  onLicenseChanged,
  appInfo = null,
}: Props) {
  const { t, locale, setLocale } = useI18n()
  const [error, setError] = useState<string | null>(null)
  const errorBannerId = useId()
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
  const [passwordErrorField, setPasswordErrorField] = useState<'current' | 'confirm' | null>(
    null,
  )
  // Single write path for the page-wide error banner: every other handler
  // (entity create/delete, auto-lock, backup, restore) shares `error` with
  // the password form, so routing all of them through here guarantees a
  // non-password error clears any stale aria-invalid left on a password
  // field by an earlier password-change failure.
  function setPageError(
    message: string | null,
    passwordField: 'current' | 'confirm' | null = null,
  ) {
    setError(message)
    setPasswordErrorField(passwordField)
  }
  const [backupBusy, setBackupBusy] = useState(false)
  const [restoreOpen, setRestoreOpen] = useState(false)
  const [restoreBusy, setRestoreBusy] = useState(false)
  const [restorePath, setRestorePath] = useState<string | undefined>(undefined)
  const [restorePicking, setRestorePicking] = useState(false)
  const [license, setLicense] = useState<LicenseStatus | null>(null)
  const [licenseError, setLicenseError] = useState<string | null>(null)
  const [licenseBusy, setLicenseBusy] = useState(false)
  const [eulaText, setEulaText] = useState('')
  const [eulaOpen, setEulaOpen] = useState(false)
  const newEntityAnchorRef = useRef<HTMLDivElement>(null)

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
    void api
      .licenseStatus()
      .then((status) => {
        setLicense(status)
        onLicenseChanged?.(status)
      })
      .catch(() => {
        /* ignore — Rust command lands on the same PR */
      })
    void api
      .eulaText()
      .then(setEulaText)
      .catch(() => {
        /* ignore — the viewer link simply stays inert */
      })
    // onLicenseChanged is intentionally excluded: this effect only fetches
    // once on mount. Forwarding it here would mean a fresh SettingsPage
    // mount (main's key={active} remounts Settings on every nav) reports
    // its still-unresolved null state upward before the fetch above
    // settles, blanking App's already-fetched TrialBanner license. Every
    // real license change is forwarded imperatively at its write site
    // instead — see onImportLicense and applyExpiredFromWrite below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // App bumps createBookIntent from the five empty-state CTAs. Pop the
  // create-entity form open and scroll to it — scrollIntoView is undefined
  // in jsdom, so guard it. Then tell App the intent was consumed, so a later
  // remount of this page (main's key={active} tears Settings down on every
  // navigation) does not replay a stale nonzero intent and reopen the
  // dialog on every subsequent visit.
  useEffect(() => {
    if (!createBookIntent) return
    setShowCreate(true)
    newEntityAnchorRef.current?.scrollIntoView?.({ behavior: 'smooth' })
    onCreateBookIntentHandled?.()
  }, [createBookIntent, onCreateBookIntentHandled])

  function applyExpiredFromWrite(): void {
    const next: LicenseStatus = {
      state: 'expired',
      days_remaining: license?.days_remaining,
      licensed_until: license?.licensed_until,
    }
    setLicense(next)
    onLicenseChanged?.(next)
    setLicenseError(null)
    setPageError(null)
  }

  function commandErrorMessage(err: unknown, fallback = ''): string | null {
    const cmd = err as CommandError
    if (isLicenseExpiredCode(cmd.code)) {
      applyExpiredFromWrite()
      return null
    }
    const licenseCopy = licenseErrorMessage(cmd.code)
    if (licenseCopy !== undefined) return licenseCopy
    return cmd.message || fallback
  }

  const addAnotherBook = canAddAnotherBook(license, entities.length)

  async function onCreate(ev: FormEvent) {
    ev.preventDefault()
    if (!canAddAnotherBook(license, entities.length)) return
    setBusy(true)
    setPageError(null)
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
      const message = commandErrorMessage(err)
      if (message !== null) setPageError(message)
    } finally {
      setBusy(false)
    }
  }

  async function confirmDelete() {
    if (!pendingDelete) return
    setDeleteBusy(true)
    setPageError(null)
    try {
      await api.entityDelete(pendingDelete.id)
      setPendingDelete(null)
      await onEntitiesChange()
    } catch (err) {
      const message = commandErrorMessage(err, t('settings.deleteFailed'))
      if (message !== null) setPageError(message)
    } finally {
      setDeleteBusy(false)
    }
  }

  async function saveLock(mins: number) {
    if (!Number.isFinite(mins) || mins < 1) {
      setPageError(t('settings.lockTimeoutMin'))
      return
    }
    setLockBusy(true)
    setPageError(null)
    try {
      const secs = Math.round(mins * 60)
      await api.setLockTimeout(secs)
      setLockMins(mins)
      onLockTimeoutChange?.(secs)
    } catch (err) {
      const message = commandErrorMessage(err)
      if (message !== null) setPageError(message)
    } finally {
      setLockBusy(false)
    }
  }

  async function onChangePassword(ev: FormEvent) {
    ev.preventDefault()
    setPageError(null)
    setNotice(null)

    if (newPassword !== confirmPassword) {
      setPageError(t('settings.passwordsMismatch'), 'confirm')
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
      if (isLicenseExpiredCode(cmd.code)) {
        applyExpiredFromWrite()
        return
      }
      const licenseCopy = licenseErrorMessage(cmd.code)
      if (licenseCopy !== undefined) {
        setPageError(licenseCopy)
        return
      }
      setPageError(
        cmd.code === 'invalid_password'
          ? t('settings.currentPasswordIncorrect')
          : cmd.message || t('settings.changePasswordFailed'),
        'current',
      )
    } finally {
      setPasswordBusy(false)
    }
  }

  async function onImportLicense() {
    setLicenseBusy(true)
    setLicenseError(null)
    try {
      const next = await api.licenseInstall()
      if (next === null) return
      setLicense(next)
      onLicenseChanged?.(next)
    } catch (err) {
      const cmd = err as CommandError
      if (isLicenseExpiredCode(cmd.code)) {
        const next: LicenseStatus = {
          state: 'expired',
          days_remaining: license?.days_remaining,
          licensed_until: license?.licensed_until,
        }
        setLicense(next)
        onLicenseChanged?.(next)
        return
      }
      setLicenseError(licenseImportError(cmd))
    } finally {
      setLicenseBusy(false)
    }
  }

  async function onBackup() {
    if (!backupEnabled) return
    setPageError(null)
    setNotice(null)
    setBackupBusy(true)
    try {
      await vaultBackup()
    } catch (err) {
      setPageError(backupCommandError(err as CommandError))
    } finally {
      setBackupBusy(false)
    }
  }

  async function beginRestore() {
    if (restoreBusy || restorePicking || restoreOpen) return
    setPageError(null)
    setRestorePicking(true)
    try {
      const path = await vaultPickBackup()
      if (path === null) return
      setRestorePath(path)
      setRestoreOpen(true)
    } catch (err) {
      setPageError(backupCommandError(err as CommandError))
    } finally {
      setRestorePicking(false)
    }
  }

  async function confirmRestore() {
    if (!restorePath) return
    setRestoreBusy(true)
    setPageError(null)
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
      setPageError(backupCommandError(err as CommandError))
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

      <ErrorBanner id={errorBannerId} message={error} />
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
      >
        <LanguagePill
          value={locale}
          onChange={setLocale}
          ariaLabel={t('settings.language.title')}
        />
      </CollapsibleSection>

      <CollapsibleSection
        title={t('settings.license.title')}
        description={t('settings.license.description')}
        icon={<Key className="size-4" />}
        defaultOpen
      >
        <div className="space-y-4">
          <div className="flex flex-wrap items-center gap-3">
            {license?.state === 'trial' && license.days_remaining != null ? (
              <div className="inline-flex items-center rounded-full bg-[var(--color-accent-soft)] px-3 py-1 text-sm font-medium text-[var(--color-accent)]">
                {t('settings.trial.banner.active', { n: license.days_remaining })}
              </div>
            ) : null}
            {license?.state === 'licensed' && license.licensed_until ? (
              <div className="inline-flex items-center rounded-full bg-[var(--color-surface-elevated)] px-3 py-1 text-sm font-medium text-[var(--color-fg-secondary)]">
                {t('settings.license.licensedUntil', {
                  date: formatLicensedUntil(license.licensed_until, locale),
                })}
              </div>
            ) : null}
            {license?.state === 'expired' ? (
              <div className="inline-flex items-center rounded-full bg-[var(--color-warning-soft)] px-3 py-1 text-sm font-medium text-[var(--color-warning)]">
                {licenseExpiredBanner(license)}
              </div>
            ) : null}
            <Button
              variant="secondary"
              busy={licenseBusy}
              onClick={() => void onImportLicense()}
            >
              <Upload className="size-3.5" />
              {license?.state === 'licensed'
                ? t('settings.license.replace')
                : t('settings.license.import')}
            </Button>
            {license && license.state !== 'licensed' && license.buy_url ? (
              <Button
                onClick={() => {
                  void openUrl(license.buy_url ?? '')
                }}
              >
                {t('settings.license.buy')}
              </Button>
            ) : null}
          </div>
          <ErrorBanner message={licenseError} className="" />
          <button
            type="button"
            className="text-xs text-[var(--color-muted)] underline-offset-2 hover:text-[var(--color-fg)] hover:underline"
            onClick={() => setEulaOpen(true)}
          >
            {t('settings.license.viewEula')}
          </button>
        </div>
      </CollapsibleSection>

      {appInfo ? (
        <CollapsibleSection
          title={t('settings.support.title')}
          description={t('settings.support.description', { email: appInfo.support_email })}
          icon={<LifeBuoy className="size-4" />}
          tone="info"
        >
          <div className="space-y-4">
            <p className="text-sm text-[var(--color-fg-secondary)]">
              {t('settings.support.body', {
                email: appInfo.support_email,
                version: appInfo.version,
              })}
            </p>
            <Button
              variant="secondary"
              onClick={() => {
                void openUrl(appInfo.support_mailto)
              }}
            >
              <Mail className="size-3.5" />
              {t('settings.support.contact')}
            </Button>
          </div>
        </CollapsibleSection>
      ) : null}

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
        <form onSubmit={onChangePassword} className="grid max-w-3xl gap-4 sm:grid-cols-3">
          <Field label={t('settings.currentPassword')}>
            <Input
              type="password"
              autoComplete="current-password"
              value={oldPassword}
              onChange={(e) => {
                setOldPassword(e.target.value)
                setPasswordErrorField(null)
              }}
              required
              aria-invalid={passwordErrorField === 'current' || undefined}
              aria-describedby={passwordErrorField === 'current' ? errorBannerId : undefined}
            />
          </Field>
          <Field label={t('settings.newPassword')}>
            <Input
              type="password"
              autoComplete="new-password"
              value={newPassword}
              onChange={(e) => {
                setNewPassword(e.target.value)
                setPasswordErrorField(null)
              }}
              required
            />
          </Field>
          <Field label={t('settings.confirmNewPassword')}>
            <Input
              type="password"
              autoComplete="new-password"
              value={confirmPassword}
              onChange={(e) => {
                setConfirmPassword(e.target.value)
                setPasswordErrorField(null)
              }}
              required
              aria-invalid={passwordErrorField === 'confirm' || undefined}
              aria-describedby={passwordErrorField === 'confirm' ? errorBannerId : undefined}
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

      <div ref={newEntityAnchorRef} />

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

      <Modal
        open={eulaOpen}
        title={t('settings.license.eulaTitle')}
        onClose={() => setEulaOpen(false)}
      >
        <pre className="max-h-[60vh] overflow-y-auto whitespace-pre-wrap text-xs leading-relaxed text-[var(--color-fg-secondary)]">
          {eulaText}
        </pre>
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
        <div className="flex flex-wrap items-center gap-3 border-t border-[var(--color-border)] px-5 py-3.5">
          {addAnotherBook ? null : (
            <p className="text-xs leading-snug text-[var(--color-muted)]">
              {t('license.entityLimitHint')}
            </p>
          )}
          <Button
            size="sm"
            className="ml-auto"
            disabled={!addAnotherBook}
            onClick={() => setShowCreate(true)}
          >
            <Plus className="size-3.5" />
            {t('settings.entities.new')}
          </Button>
        </div>
      </CollapsibleSection>
    </div>
  )
}
