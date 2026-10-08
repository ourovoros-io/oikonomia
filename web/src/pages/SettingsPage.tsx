import { useEffect, useId, useRef, useState, type FormEvent } from 'react'
import {
  Archive,
  ArchiveRestore,
  Briefcase,
  Building2,
  Clock,
  Download,
  FileQuestion,
  HeartHandshake,
  KeyRound,
  Languages,
  LifeBuoy,
  Mail,
  PencilLine,
  Plus,
  Timer,
  Trash2,
  Upload,
  User,
} from 'lucide-react'
import { DonationAddresses } from '../components/DonationAddresses'
import { SupportMailFallback } from '../components/SupportMailFallback'
import { api, type ChartTemplate, type DonationAddress, type Entity } from '../lib/api'
import {
  vaultBackup,
  vaultChangePassword,
  vaultPickBackup,
  vaultRestore,
  type AppInfo,
} from '../lib/tauri'
import { asCommandError, commandErrorMessage } from '../lib/commandError'
import { cn } from '../lib/cn'
import { CURRENCIES } from '../lib/currencies'
import { ConfirmDialog } from '../components/ConfirmDialog'
import { Modal } from '../components/Modal'
import { RenameDialog } from '../components/RenameDialog'
import { TopBar } from '../components/TopBar'
import {
  Button,
  ChoiceCard,
  CollapsibleSection,
  ErrorBanner,
  Field,
  IconBadge,
  Input,
  Notice,
  Select,
  ToastStack,
} from '../components/ui'
import {
  backupCommandError,
  restoreCommandError,
  canBackupVault,
  restoreConfirm,
  vaultBackupAvailability,
  vaultBackupBanner,
  vaultBackupBody,
  vaultBackupHint,
} from '../lib/vaultBackupUi'
import { useI18n } from '../lib/I18nProvider'
import { LanguagePill } from '../components/LanguagePill'
import { rememberBookCurrency, rememberedBookCurrency } from '../lib/sessionDefaults'
import { useDialogError } from '../lib/useDialogError'

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

const LOCK_PRESETS = [
  { mins: 5, labelKey: 'settings.lock.5min' },
  { mins: 15, labelKey: 'settings.lock.15min' },
  { mins: 30, labelKey: 'settings.lock.30min' },
  { mins: 60, labelKey: 'settings.lock.1hour' },
] as const

/** The password field a refusal from Rust is about, so that field is the one outlined. */
function passwordFieldForCode(code: string | undefined): 'current' | 'new' | null {
  if (code === 'invalid_password') return 'current'
  if (code === 'password_too_short') return 'new'

  return null
}

export function SettingsPage({
  entities,
  vaultPresent = true,
  onEntitiesChange,
  onSelectEntity,
  onLockTimeoutChange,
  createBookIntent,
  onCreateBookIntentHandled,
  appInfo = null,
}: Props) {
  const { t, locale, setLocale, languageChangeFailed } = useI18n()
  const errorBannerId = useId()
  // Which confirmation to show, by catalog key, so it follows a language change.
  const [noticeKey, setNoticeKey] = useState<
    'settings.prefs.resetDone' | 'settings.passwordChanged' | 'settings.lock.saved' | null
  >(null)
  // The name of the book just created, for its confirmation.
  const [createdBook, setCreatedBook] = useState<string | null>(null)
  /** Set once "Email support" was clicked: the opener cannot confirm a mail app opened. */
  const [mailTried, setMailTried] = useState(false)
  const [name, setName] = useState('')
  const [currency, setCurrency] = useState(rememberedBookCurrency)
  const [template, setTemplate] = useState<ChartTemplate>('personal')
  const [busy, setBusy] = useState(false)
  const [showCreate, setShowCreate] = useState(false)
  const [lockMins, setLockMins] = useState(15)
  const [lockBusy, setLockBusy] = useState(false)
  const [donations, setDonations] = useState<DonationAddress[]>([])
  const [pendingDelete, setPendingDelete] = useState<{ id: string; name: string } | null>(null)
  // A failure is drawn in the dialog that is open, not on the page behind its scrim.
  const openDialog = showCreate ? 'create' : pendingDelete ? 'delete' : null
  const [error, setError] = useDialogError(openDialog)
  const dismissError = () => setError(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const [archived, setArchived] = useState<Entity[]>([])
  // Bumped after every change that can move a book in or out of the archive.
  const [archivedVersion, setArchivedVersion] = useState(0)
  const archivedHeadingId = useId()
  const [pendingArchive, setPendingArchive] = useState<{ id: string; name: string } | null>(null)
  const [archiveBusy, setArchiveBusy] = useState(false)
  const [restoringId, setRestoringId] = useState<string | null>(null)
  const [prefsUnreadable, setPrefsUnreadable] = useState(false)
  const [prefsResetBusy, setPrefsResetBusy] = useState(false)
  const [oldPassword, setOldPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [passwordBusy, setPasswordBusy] = useState(false)
  // Drawn inside the password form, beside the field it names, not at the top of the page.
  const [passwordProblem, setPasswordProblem] = useState<{
    text: string
    field: 'current' | 'new' | 'confirm' | null
  } | null>(null)
  function setPageError(message: string | null) {
    setError(message)
  }
  const [backupBusy, setBackupBusy] = useState(false)
  const [restoreOpen, setRestoreOpen] = useState(false)
  const [restoreBusy, setRestoreBusy] = useState(false)
  const [restorePath, setRestorePath] = useState<string | undefined>(undefined)
  const [restorePicking, setRestorePicking] = useState(false)
  const newEntityAnchorRef = useRef<HTMLDivElement>(null)
  // The book list is open when the page was opened to add a book, so the new
  // book is in view; otherwise it starts folded like the other sections.
  const [entitiesOpenAtStart] = useState(() => Boolean(createBookIntent))
  const [newBookId, setNewBookId] = useState<string | null>(null)
  const [renameTarget, setRenameTarget] = useState<Entity | null>(null)

  const backupAvailability = vaultBackupAvailability({
    vaultPresent,
    entityCount: entities.length,
  })
  const backupBanner = vaultBackupBanner(backupAvailability)
  const backupEnabled = canBackupVault(backupAvailability)
  const replaceConfirm = restoreConfirm('replace')
  const lockPreset = LOCK_PRESETS.find((p) => p.mins === lockMins)
  const lockSummary = lockPreset ? t(lockPreset.labelKey) : undefined

  useEffect(() => {
    void api
      .getLockTimeout()
      .then((secs) => setLockMins(Math.max(1, Math.round(secs / 60))))
      .catch(() => {
        /* ignore */
      })

    void api
      .donationAddresses()
      .then(setDonations)
      .catch(() => {
        /* ignore: the section stays hidden */
      })
  }, [])

  // Whether the preferences file can be read is asked, not learned from a
  // failed save. A language change that failed is one reason for it to have
  // changed, so that asks again.
  useEffect(() => {
    let cancelled = false

    async function readPrefsState() {
      try {
        const prefs = await api.getUiPrefs()
        if (!cancelled) setPrefsUnreadable(prefs.unreadable)
      } catch {
        /* The state is unknown, so the notice stays as it was. */
      }
    }
    void readPrefsState()

    return () => {
      cancelled = true
    }
  }, [languageChangeFailed])

  // Archived books are not in the list App holds, which is the books that
  // can be opened, so this page asks for them itself.
  useEffect(() => {
    if (!vaultPresent) return
    let cancelled = false

    async function loadArchived() {
      try {
        const books = await api.entityListArchived()
        if (!cancelled) setArchived(books)
      } catch (err) {
        if (cancelled) return
        setError(commandErrorMessage(err, 'settings.entities.archived.loadError'))
      }
    }
    void loadArchived()

    return () => {
      cancelled = true
    }
  }, [vaultPresent, archivedVersion, setError])

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

  async function onCreate(ev: FormEvent) {
    ev.preventDefault()
    if (!name.trim()) {
      setPageError(t('error.nameRequired'))
      return
    }
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
      // Stay here: adding several books should not take a trip to each
      // one's Dashboard. The new book is marked in the list instead.
      setNewBookId(entity.id)
      setCreatedBook(entity.name)
    } catch (err) {
      setPageError(commandErrorMessage(err))
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
      setPageError(commandErrorMessage(err, 'settings.deleteFailed'))
    } finally {
      setDeleteBusy(false)
    }
  }

  // Archiving the current book needs nothing more than deleting it does:
  // App reloads the active books and falls to the first of them, or to the
  // no-book state when none is left.
  async function confirmArchive() {
    if (!pendingArchive) return
    setArchiveBusy(true)
    setPageError(null)
    setNoticeKey(null)
    try {
      await api.entityArchive(pendingArchive.id)
      await onEntitiesChange()
    } catch (err) {
      setPageError(commandErrorMessage(err, 'settings.entities.archiveError'))
    } finally {
      // Closed on failure too, so the error is not behind the dialog.
      setPendingArchive(null)
      setArchiveBusy(false)
      setArchivedVersion((version) => version + 1)
    }
  }

  async function onRestore(book: Entity) {
    setRestoringId(book.id)
    setPageError(null)
    setNoticeKey(null)
    try {
      await api.entityUnarchive(book.id)
      await onEntitiesChange()
    } catch (err) {
      const cmd = asCommandError(err)
      // The general sentence for a taken name says to choose another, which
      // an archived book cannot do: it is read-only.
      setPageError(
        cmd.code === 'name_taken'
          ? t('settings.entities.restoreNameTaken', { name: book.name })
          : commandErrorMessage(cmd, 'settings.entities.restoreError'),
      )
    } finally {
      setRestoringId(null)
      setArchivedVersion((version) => version + 1)
    }
  }

  async function onResetPrefs() {
    setPrefsResetBusy(true)
    setPageError(null)
    setNoticeKey(null)
    try {
      const prefs = await api.resetUiPrefs()
      setPrefsUnreadable(prefs.unreadable)
      if (!prefs.unreadable) setNoticeKey('settings.prefs.resetDone')
    } catch (err) {
      setPageError(commandErrorMessage(err, 'settings.prefs.resetFailed'))
    } finally {
      setPrefsResetBusy(false)
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
      setNoticeKey('settings.lock.saved')
      onLockTimeoutChange?.(secs)
    } catch (err) {
      setPageError(commandErrorMessage(err))
    } finally {
      setLockBusy(false)
    }
  }

  // Fixing the field the complaint names ends the complaint.
  function clearPasswordProblem(field: 'current' | 'new' | 'confirm') {
    setPasswordProblem((prev) => (prev?.field === field ? null : prev))
  }

  async function onChangePassword(ev: FormEvent) {
    ev.preventDefault()
    setPasswordProblem(null)
    setNoticeKey(null)

    if (!oldPassword || !newPassword) {
      setPasswordProblem({
        text: t('form.fieldRequired'),
        field: oldPassword ? 'new' : 'current',
      })
      return
    }
    if (newPassword !== confirmPassword) {
      setPasswordProblem({ text: t('settings.passwordsMismatch'), field: 'confirm' })
      return
    }
    // Password strength rules live in Rust; its Validation error surfaces below.

    setPasswordBusy(true)
    try {
      await vaultChangePassword(oldPassword, newPassword)
      setOldPassword('')
      setNewPassword('')
      setConfirmPassword('')
      setNoticeKey('settings.passwordChanged')
    } catch (err) {
      // A rejection can be anything, including nothing.
      const cmd = asCommandError(err)
      setPasswordProblem({
        text:
          cmd.code === 'invalid_password'
            ? t('settings.currentPasswordIncorrect')
            : commandErrorMessage(cmd, 'settings.changePasswordFailed'),
        field: passwordFieldForCode(cmd.code),
      })
    } finally {
      setPasswordBusy(false)
    }
  }

  async function onEmailSupport() {
    if (!appInfo) return
    setMailTried(true)
    try {
      await api.openSupportEmail()
    } catch {
      // No associated mail client, or the OS refused. The address is on
      // screen already, so the fallback copy just points at it.
      setPageError(t('settings.support.openFailed', { email: appInfo.support_email }))
    }
  }

  async function onBackup() {
    if (!backupEnabled) return
    setPageError(null)
    setNoticeKey(null)
    setBackupBusy(true)
    try {
      await vaultBackup()
    } catch (err) {
      setPageError(backupCommandError(err))
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
      setPageError(restoreCommandError(err))
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
      setPageError(restoreCommandError(err))
      setRestoreOpen(false)
      setRestorePath(undefined)
    } finally {
      setRestoreBusy(false)
    }
  }

  return (
    <div className="space-y-4">
      <TopBar title={t('settings.title')} subtitle={t('settings.description')} />

      <ToastStack>
        <ErrorBanner className="" message={openDialog ? null : error} onDismiss={dismissError} />
        <Notice
          message={noticeKey ? t(noticeKey) : null}
          onDismiss={() => setNoticeKey(null)}
        />
        <Notice
          message={createdBook ? t('settings.entities.created', { name: createdBook }) : null}
          onDismiss={() => setCreatedBook(null)}
        />
      </ToastStack>
      {prefsUnreadable ? (
        <div
          role="status"
          className="flex flex-wrap items-center gap-3 rounded-xl border border-[var(--color-warning)]/25 bg-[var(--color-warning-soft)] px-4 py-3 text-sm text-[var(--color-fg-secondary)]"
        >
          <p className="min-w-0 flex-1">{t('settings.prefs.unreadable')}</p>
          <Button
            variant="secondary"
            size="sm"
            busy={prefsResetBusy}
            onClick={() => void onResetPrefs()}
          >
            {t('settings.prefs.reset')}
          </Button>
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
      >
        <ErrorBanner
          message={openDialog === 'delete' ? error : null}
          className=""
          onDismiss={dismissError}
        />
      </ConfirmDialog>

      <ConfirmDialog
        open={pendingArchive !== null}
        title={t('settings.entities.archiveConfirm.title')}
        body={
          pendingArchive
            ? t('settings.entities.archiveConfirm.body', { name: pendingArchive.name })
            : ''
        }
        confirmLabel={t('settings.entities.archiveTitle')}
        busy={archiveBusy}
        onCancel={() => {
          if (!archiveBusy) setPendingArchive(null)
        }}
        onConfirm={() => void confirmArchive()}
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
        {languageChangeFailed ? (
          <p role="alert" className="mt-3 text-sm text-[var(--color-danger)]">
            {t('settings.language.error')}
          </p>
        ) : null}
      </CollapsibleSection>

      {donations.length > 0 ? (
        <CollapsibleSection
          title={t('settings.donate.title')}
          description={t('settings.donate.description')}
          icon={<HeartHandshake className="size-4" />}
          tone="muted"
        >
          <DonationAddresses addresses={donations} />
        </CollapsibleSection>
      ) : null}

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
            <Button variant="secondary" onClick={() => void onEmailSupport()}>
              <Mail className="size-3.5" />
              {t('settings.support.contact')}
            </Button>
            {mailTried ? <SupportMailFallback email={appInfo.support_email} /> : null}
          </div>
        </CollapsibleSection>
      ) : null}

      <CollapsibleSection
        title={t('settings.autoLock.title')}
        description={t('settings.autoLock.description')}
        icon={<Timer className="size-4" />}
        tone="warning"
        summary={lockSummary}
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
        <form noValidate onSubmit={onChangePassword} className="grid max-w-3xl gap-4 sm:grid-cols-3">
          <Field label={t('settings.currentPassword')}>
            <Input
              type="password"
              autoComplete="current-password"
              value={oldPassword}
              onChange={(e) => {
                setOldPassword(e.target.value)
                clearPasswordProblem('current')
              }}
              required
              aria-invalid={passwordProblem?.field === 'current' || undefined}
              aria-describedby={passwordProblem?.field === 'current' ? errorBannerId : undefined}
            />
          </Field>
          <Field label={t('settings.newPassword')}>
            <Input
              type="password"
              autoComplete="new-password"
              value={newPassword}
              onChange={(e) => {
                setNewPassword(e.target.value)
                clearPasswordProblem('new')
              }}
              required
              aria-invalid={passwordProblem?.field === 'new' || undefined}
              aria-describedby={passwordProblem?.field === 'new' ? errorBannerId : undefined}
            />
          </Field>
          <Field label={t('settings.confirmNewPassword')}>
            <Input
              type="password"
              autoComplete="new-password"
              value={confirmPassword}
              onChange={(e) => {
                setConfirmPassword(e.target.value)
                clearPasswordProblem('confirm')
              }}
              required
              aria-invalid={passwordProblem?.field === 'confirm' || undefined}
              aria-describedby={passwordProblem?.field === 'confirm' ? errorBannerId : undefined}
            />
          </Field>
          <div className="sm:col-span-3">
            <ErrorBanner
              id={errorBannerId}
              message={passwordProblem?.text ?? null}
              className="mb-4"
              onDismiss={() => setPasswordProblem(null)}
            />
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

      <RenameDialog
        current={renameTarget?.name ?? null}
        title={t('settings.entities.rename.title')}
        label={t('settings.newEntity.name')}
        onSave={async (nextName) => {
          if (!renameTarget) return
          await api.entityUpdate(renameTarget.id, nextName)
          await onEntitiesChange()
        }}
        onClose={() => setRenameTarget(null)}
      />

      <Modal
        open={showCreate}
        title={t('settings.newEntity.title')}
        description={t('settings.newEntity.description')}
        error={openDialog === 'create' ? error : null}
        onDismissError={dismissError}
        onClose={() => {
          if (!busy) setShowCreate(false)
        }}
      >
        <form noValidate onSubmit={onCreate} className="space-y-4">
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
              <Select
                value={currency}
                onChange={(e) => {
                  setCurrency(e.target.value)
                  rememberBookCurrency(e.target.value)
                }}
                required
              >
                {CURRENCIES.map((c) => (
                  <option key={c.code} value={c.code}>
                    {c.code} — {t(`currency.${c.code}`)}
                  </option>
                ))}
              </Select>
            </Field>
          </div>

          <div>
            <span className="mb-1.5 block font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
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
        tone="accent"
        flush
        defaultOpen={entitiesOpenAtStart}
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
                aria-current={e.id === newBookId ? 'true' : undefined}
                className={cn(
                  'flex items-center gap-3 px-5 py-3 transition hover:bg-[var(--color-surface-2)]/50',
                  e.id === newBookId && 'bg-[var(--color-accent-soft)]',
                )}
              >
                {/* Same tile and gap as the section header, so names start under its title. */}
                <IconBadge tone="accent" size="sm">
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
                    variant="secondary"
                    size="iconSm"
                    onClick={() => setRenameTarget(e)}
                    aria-label={t('settings.entities.rename.aria', { name: e.name })}
                    title={t('common.rename')}
                  >
                    <PencilLine className="size-3.5" />
                  </Button>
                  <Button
                    variant="secondary"
                    size="iconSm"
                    onClick={() => setPendingArchive({ id: e.id, name: e.name })}
                    aria-label={t('settings.entities.archiveAria', { name: e.name })}
                  >
                    <Archive className="size-3.5" />
                  </Button>
                  <Button
                    variant="danger"
                    size="iconSm"
                    // Apart from Archive: the destructive one is not a neighbour to mis-hit.
                    className="ml-2"
                    onClick={() => setPendingDelete({ id: e.id, name: e.name })}
                    aria-label={t('settings.entities.deleteAria', { name: e.name })}
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
        <div className="flex flex-wrap items-center gap-3 border-t border-[var(--color-border)] px-5 py-3">
          <Button size="sm" className="ml-auto" onClick={() => setShowCreate(true)}>
            <Plus className="size-3.5" />
            {t('settings.entities.new')}
          </Button>
        </div>
        {archived.length > 0 ? (
          <section
            aria-labelledby={archivedHeadingId}
            className="border-t border-[var(--color-border)]"
          >
            <div className="px-5 pt-4 pb-2">
              <h4
                id={archivedHeadingId}
                className="font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase"
              >
                {t('settings.entities.archived.title')}
              </h4>
              <p className="mt-1 text-xs text-[var(--color-muted)]">
                {t('settings.entities.archived.hint')}
              </p>
            </div>
            <ul className="divide-y divide-[var(--color-border)]">
              {archived.map((book) => (
                <li key={book.id} className="flex items-center gap-3 px-5 py-3">
                  <IconBadge tone="muted" size="sm">
                    <Archive className="size-4" />
                  </IconBadge>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-[var(--color-fg-secondary)]">
                      {book.name}
                    </div>
                    <div className="text-xs text-[var(--color-muted)]">
                      <span className="tabular-nums">{book.base_currency}</span>
                      <span className="mx-1.5 text-[var(--color-border-strong)]">·</span>
                      <span>{t(`chart.${book.chart_template}`)}</span>
                    </div>
                  </div>
                  <Button
                    variant="secondary"
                    size="sm"
                    className="shrink-0"
                    busy={restoringId === book.id}
                    disabled={restoringId !== null}
                    onClick={() => void onRestore(book)}
                    aria-label={t('settings.entities.restoreAria', { name: book.name })}
                  >
                    {restoringId === book.id ? null : <ArchiveRestore className="size-3.5" />}
                    {t('settings.entities.restore')}
                  </Button>
                </li>
              ))}
            </ul>
          </section>
        ) : null}
      </CollapsibleSection>
    </div>
  )
}
