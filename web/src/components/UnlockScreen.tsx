import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type FormEvent,
} from 'react'
import { KeyRound } from 'lucide-react'
import { cn } from '../lib/cn'
import { Logo } from './Logo'
import { ConfirmDialog } from './ConfirmDialog'
import { wholePixelShift } from '../lib/wholePixel'
import type { VaultStatus } from '../lib/tauri'
import {
  vaultInit,
  vaultPickBackup,
  vaultRestore,
  vaultStatus,
  vaultUnlock,
} from '../lib/tauri'
import { asCommandError, commandErrorMessage } from '../lib/commandError'
import { restoreCommandError, restoreConfirm } from '../lib/vaultBackupUi'
import { useI18n } from '../lib/I18nProvider'
import { LanguagePill } from './LanguagePill'
import { UnlockUpdateDialog } from './UnlockUpdateDialog'
import { useUnlockUpdate } from './useUnlockUpdate'
import { Button, ErrorBanner, Field, Input, Notice } from './ui'

type Props = {
  status: Exclude<VaultStatus, 'unlocked'>
  onUnlocked: (status: VaultStatus) => void
  /** Support mailbox from Rust `app_info`, so a locked-out user still has somewhere to write. */
  supportEmail?: string | null
  /** Running version from Rust `app_info`, shown on the up-to-date frame. */
  appVersion?: string | null
}

// Long enough for the logo's oik-logo-pulse to finish, short enough
// that unlocking never feels slower.
const SUCCESS_BEAT_MS = 420

// Rounded, so the shared focus ring hugs the text instead of drawing a tight square.
const footerLink = [
  'h-6 rounded-md px-1.5 text-[13px] font-medium text-[var(--color-muted)]',
  'hover:text-[var(--color-fg-secondary)] disabled:opacity-50',
].join(' ')

const fieldIcon = [
  'pointer-events-none absolute top-1/2 left-0 size-4 -translate-y-1/2',
  'text-[var(--color-muted)]',
].join(' ')

export function UnlockScreen({
  status,
  onUnlocked,
  supportEmail = null,
  appVersion = null,
}: Props) {
  const { t, locale, setLocale, languageChangeFailed } = useI18n()
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [restoreOpen, setRestoreOpen] = useState(false)
  const [restoreBusy, setRestoreBusy] = useState(false)
  const [restorePath, setRestorePath] = useState<string | undefined>(undefined)
  const [restorePicking, setRestorePicking] = useState(false)
  const [unlocking, setUnlocking] = useState(false)
  const [shaking, setShaking] = useState(false)
  // Scoped separately from `error`: the banner is shared with the restore
  // flow (beginRestore/confirmRestore), which must never mark the password
  // fields invalid — only a failed unlock/create submit does.
  const [passwordInvalid, setPasswordInvalid] = useState(false)
  const passwordRef = useRef<HTMLInputElement>(null)
  const confirmRef = useRef<HTMLInputElement>(null)
  const columnRef = useRef<HTMLDivElement>(null)
  const handoffTimer = useRef<number | undefined>(undefined)
  const errorId = useId()
  const updateFlow = useUnlockUpdate()

  const isSetup = status === 'uninitialized'
  const restorePrompt = restoreConfirm(isSetup ? 'load' : 'replace')

  useEffect(() => () => window.clearTimeout(handoffTimer.current), [])

  // m-auto centres on a fractional leftover. Shift by under half a pixel so
  // the card's top is a whole pixel, before paint, so nothing appears to move.
  useLayoutEffect(() => {
    const column = columnRef.current
    if (!column) return

    const snap = () => {
      column.style.translate = ''
      const shift = wholePixelShift(column.getBoundingClientRect().top)
      if (shift !== 0) column.style.translate = `0 ${shift}px`
    }

    snap()
    if (typeof ResizeObserver === 'undefined') return
    const parent = column.parentElement
    const observer = new ResizeObserver(snap)
    observer.observe(column)
    if (parent) observer.observe(parent)
    return () => observer.disconnect()
  }, [status, locale])

  // Typing starts at once on arrival, and after a restore turns "create" into
  // "unlock", when the field would otherwise have lost focus.
  useEffect(() => {
    passwordRef.current?.focus()
  }, [status])

  // A dialog over the form hands focus back to the button that opened it, not
  // to the field the person was about to type in.
  const dialogOpen = restoreOpen || updateFlow.update.kind !== 'idle'
  const dialogWasOpen = useRef(false)
  useEffect(() => {
    if (dialogWasOpen.current && !dialogOpen) passwordRef.current?.focus()
    dialogWasOpen.current = dialogOpen
  }, [dialogOpen])

  /** Marks the fields as wrong, shakes the card and puts the caret in `field`. */
  function rejectInput(message: string, field: 'password' | 'confirm') {
    setError(message)
    setPasswordInvalid(true)
    setShaking(true)
    const target = field === 'confirm' ? confirmRef.current : passwordRef.current
    // After the render that clears the cleared field, or the caret lands in a stale one.
    window.setTimeout(() => target?.focus(), 0)
  }

  async function onSubmit(event: FormEvent) {
    event.preventDefault()
    if (unlocking) return
    setError(null)
    setPasswordInvalid(false)

    if (!password) {
      rejectInput(t('form.passwordRequired'), 'password')
      return
    }
    if (isSetup && password !== confirm) {
      rejectInput(t('unlock.passwordsMismatch'), 'confirm')
      return
    }

    // Password strength rules live in Rust; its Validation error surfaces below.
    setBusy(true)
    let accepted = false
    try {
      const next = isSetup ? await vaultInit(password) : await vaultUnlock(password)
      accepted = true
      // Success beat: let the logo pulse once before the app takes over.
      setUnlocking(true)
      handoffTimer.current = window.setTimeout(() => onUnlocked(next), SUCCESS_BEAT_MS)
    } catch (err) {
      // A rejection can be anything, including nothing.
      const cmd = asCommandError(err)
      rejectInput(
        cmd.code === 'invalid_password'
          ? t('unlock.incorrectPassword')
          : commandErrorMessage(cmd, 'unlock.unlockFailed'),
        'password',
      )
    } finally {
      setBusy(false)
      // A wrong unlock password is retyped; a refused new vault keeps both
      // entries, so a "too short" is fixed by editing, not by typing twice again.
      if (accepted || !isSetup) setPassword('')
      if (accepted) setConfirm('')
    }
  }

  async function beginRestore() {
    if (busy || unlocking || restoreBusy || restorePicking || restoreOpen) return
    setError(null)
    setPasswordInvalid(false)
    setRestorePicking(true)
    try {
      const path = await vaultPickBackup()
      if (path === null) return
      setRestorePath(path)
      setRestoreOpen(true)
    } catch (err) {
      setError(restoreCommandError(err))
    } finally {
      setRestorePicking(false)
    }
  }

  async function confirmRestore() {
    if (!restorePath) return
    setRestoreBusy(true)
    setError(null)
    setPasswordInvalid(false)
    try {
      const result = await vaultRestore({ path: restorePath, replace: restorePrompt.replace })
      if (result === null) {
        setRestoreOpen(false)
        setRestorePath(undefined)
        return
      }
      const next = await vaultStatus()
      setRestoreOpen(false)
      setRestorePath(undefined)
      onUnlocked(next)
    } catch (err) {
      setError(restoreCommandError(err))
      setRestoreOpen(false)
      setRestorePath(undefined)
    } finally {
      setRestoreBusy(false)
    }
  }

  // m-auto instead of items-center: when the window is shorter than the
  // form, auto margins collapse and the top stays reachable by scrolling.
  return (
    <div className="relative flex h-full flex-col bg-[var(--color-canvas)]">
      <div className="flex min-h-0 flex-1 overflow-y-auto px-4">
        <div ref={columnRef} className="m-auto w-full max-w-md pt-8 pb-24">
          <div className="mb-8 flex flex-col items-center text-center">
            <Logo
              animateIn
              className={cn(
                'mb-4 size-14 rounded-2xl shadow-lg shadow-[var(--color-accent)]/20',
                unlocking && 'oik-logo-pulse',
              )}
            />
            {/* Sofia Sans for every language: Barlow has no Greek, so the title would
                change typeface when the language switches. */}
            <h1
              className={cn(
                'font-[family-name:var(--font-display)] text-2xl font-semibold',
                'tracking-tight',
              )}
            >
              {isSetup ? t('unlock.titleCreate') : t('unlock.titleWelcome')}
            </h1>
            {/* Two lines tall in every language, so a longer translation cannot move the card. */}
            <p className="mt-2 min-h-10 max-w-sm text-sm leading-5 text-[var(--color-muted)]">
              {isSetup ? t('unlock.bodyCreate') : t('unlock.bodyWelcome')}
            </p>
          </div>

          <div className="relative">
          <form
            noValidate
            className={cn(
              'glass-pane rounded-[24px] p-6',
              shaking && 'oik-shake',
            )}
            onSubmit={onSubmit}
            onAnimationEnd={(event) => {
              // Clear the flag so the next failed attempt can shake again.
              if (event.animationName === 'oik-shake') setShaking(false)
            }}
          >
            <div className="space-y-4">
              <Field label={t('unlock.password')}>
                <div className="relative">
                  {/* The icon starts on the label's edge; the text follows one gap later. */}
                  <KeyRound className={fieldIcon} />
                  <Input
                    ref={passwordRef}
                    type="password"
                    autoComplete={isSetup ? 'new-password' : 'current-password'}
                    value={password}
                    onChange={(e) => {
                      setPassword(e.target.value)
                      setPasswordInvalid(false)
                    }}
                    className="pl-6"
                    data-adorned
                    required
                    autoFocus
                    aria-invalid={passwordInvalid || undefined}
                    aria-describedby={passwordInvalid ? errorId : undefined}
                  />
                </div>
              </Field>

              {isSetup ? (
                <Field label={t('unlock.confirmPassword')}>
                  <div className="relative">
                    <KeyRound className={fieldIcon} />
                    <Input
                      ref={confirmRef}
                      type="password"
                      autoComplete="new-password"
                      value={confirm}
                      onChange={(e) => {
                        setConfirm(e.target.value)
                        setPasswordInvalid(false)
                      }}
                      className="pl-6"
                      data-adorned
                      required
                      aria-invalid={passwordInvalid || undefined}
                      aria-describedby={passwordInvalid ? errorId : undefined}
                    />
                  </div>
                </Field>
              ) : null}

              <Button type="submit" size="lg" busy={busy || unlocking} className="w-full">
                {busy || unlocking
                  ? t('common.working')
                  : isSetup
                    ? t('unlock.createVault')
                    : t('unlock.unlock')}
              </Button>
            </div>
          </form>

          {/* Below the card, out of flow: an error must not push the fields
              and the Unlock button down. */}
          <div className="absolute inset-x-0 top-full mt-3">
            <ErrorBanner
              id={errorId}
              message={error}
              className="text-center"
              onDismiss={() => setError(null)}
            />
          </div>
          </div>
        </div>
      </div>

      <div className="flex shrink-0 flex-col items-center gap-2 px-4 pt-2 pb-6">
        {/* The first book's account names are written in this language for good. */}
        <LanguagePill
          value={locale}
          onChange={setLocale}
          ariaLabel={t('settings.language.title')}
        />
        {languageChangeFailed ? (
          <p role="alert" className="text-[12px] text-[var(--color-danger)]">
            {t('settings.language.error')}
          </p>
        ) : null}
        <button
          type="button"
          className={footerLink}
          onClick={updateFlow.beginCheck}
          disabled={updateFlow.busy}
          aria-busy={updateFlow.busy}
        >
          {t('unlock.update.button')}
        </button>
        <button
          type="button"
          className={footerLink}
          onClick={() => void beginRestore()}
          disabled={busy || restoreBusy || restorePicking}
        >
          {t('unlock.restoreFromBackup')}
        </button>
        {supportEmail ? (
          <p className="mt-2 text-[12px] text-[var(--color-muted)]">
            {t('unlock.support', { email: supportEmail })}
          </p>
        ) : null}
      </div>

      {updateFlow.notice ? (
        <div className="pointer-events-none absolute inset-x-0 top-6 z-30 flex justify-center px-4">
          <div className="oik-toast w-full max-w-md">
            <Notice
              message={t('unlock.update.updated.notice', { version: updateFlow.notice.to })}
              onDismiss={updateFlow.dismissNotice}
            />
          </div>
        </div>
      ) : null}

      <UnlockUpdateDialog
        state={updateFlow.update}
        appVersion={appVersion}
        onDismiss={updateFlow.dismiss}
        onInstall={updateFlow.install}
        onResolve={updateFlow.resolvePoll}
        onCap={updateFlow.capPoll}
      />

      <ConfirmDialog
        open={restoreOpen}
        title={restorePrompt.title}
        body={restorePrompt.body}
        confirmLabel={restorePrompt.confirmLabel}
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
    </div>
  )
}
