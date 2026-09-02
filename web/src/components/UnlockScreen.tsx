import { useEffect, useId, useRef, useState, type FormEvent } from 'react'
import { KeyRound } from 'lucide-react'
import { cn } from '../lib/cn'
import { Logo } from './Logo'
import { ConfirmDialog } from './ConfirmDialog'
import { useDialogFocus } from './useDialogFocus'
import {
  isAvailableUpdate,
  readDevUnlockUpdatePreview,
  type UpdateUiState,
} from '../lib/updateCheck'
import type { VaultStatus } from '../lib/tauri'
import {
  updateCheck,
  updateInstall,
  vaultInit,
  vaultPickBackup,
  vaultRestore,
  vaultStatus,
  vaultUnlock,
  type CommandError,
} from '../lib/tauri'
import { backupCommandError, restoreConfirm } from '../lib/vaultBackupUi'
import { useI18n } from '../lib/I18nProvider'
import { Button, ErrorBanner, Field, Input } from './ui'

type Props = {
  status: Exclude<VaultStatus, 'unlocked'>
  onUnlocked: (status: VaultStatus) => void
}

const IDLE: UpdateUiState = { kind: 'idle' }

// Long enough for the logo's oik-logo-pulse to finish, short enough
// that unlocking never feels slower.
const SUCCESS_BEAT_MS = 420

export function UnlockScreen({ status, onUnlocked }: Props) {
  const { t } = useI18n()
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [restoreOpen, setRestoreOpen] = useState(false)
  const [restoreBusy, setRestoreBusy] = useState(false)
  const [restorePath, setRestorePath] = useState<string | undefined>(undefined)
  const [restorePicking, setRestorePicking] = useState(false)
  const [update, setUpdate] = useState<UpdateUiState>(
    () => readDevUnlockUpdatePreview() ?? IDLE,
  )
  const [unlocking, setUnlocking] = useState(false)
  const [shaking, setShaking] = useState(false)
  const checkGeneration = useRef(0)
  const handoffTimer = useRef<number | undefined>(undefined)
  const errorId = useId()

  const isSetup = status === 'uninitialized'
  const restorePrompt = restoreConfirm(isSetup ? 'load' : 'replace')

  useEffect(() => () => window.clearTimeout(handoffTimer.current), [])

  async function onSubmit(event: FormEvent) {
    event.preventDefault()
    if (unlocking) return
    setError(null)

    if (isSetup && password !== confirm) {
      setError(t('unlock.passwordsMismatch'))
      setShaking(true)
      return
    }

    // Password strength rules live in Rust; its Validation error surfaces below.
    setBusy(true)
    try {
      const next = isSetup ? await vaultInit(password) : await vaultUnlock(password)
      // Success beat: let the logo pulse once before the app takes over.
      setUnlocking(true)
      handoffTimer.current = window.setTimeout(() => onUnlocked(next), SUCCESS_BEAT_MS)
    } catch (err) {
      const cmd = err as CommandError
      setError(
        cmd.code === 'invalid_password'
          ? t('unlock.incorrectPassword')
          : cmd.message || t('unlock.unlockFailed'),
      )
      setShaking(true)
    } finally {
      setBusy(false)
      setPassword('')
      setConfirm('')
    }
  }

  async function beginRestore() {
    if (busy || unlocking || restoreBusy || restorePicking || restoreOpen) return
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
      setError(backupCommandError(err as CommandError))
      setRestoreOpen(false)
      setRestorePath(undefined)
    } finally {
      setRestoreBusy(false)
    }
  }

  async function beginUpdateCheck() {
    if (update.kind === 'checking' || update.kind === 'installing') return
    const generation = ++checkGeneration.current
    setUpdate({ kind: 'checking' })
    try {
      const result = await updateCheck()
      if (generation !== checkGeneration.current) return
      setUpdate(result)
    } catch {
      if (generation !== checkGeneration.current) return
      setUpdate({ kind: 'failed' })
    }
  }

  function dismissUpdate() {
    if (update.kind === 'installing') return
    checkGeneration.current += 1
    setUpdate(IDLE)
  }

  async function installAvailable() {
    if (!isAvailableUpdate(update)) return
    const available = update
    setUpdate({ kind: 'installing' })
    try {
      const result = await updateInstall(available)
      if (result?.kind === 'failed') {
        setUpdate({ kind: 'failed' })
      }
    } catch {
      setUpdate({ kind: 'failed' })
    }
  }

  // m-auto instead of items-center: when the window is shorter than the
  // form, auto margins collapse and the top stays reachable by scrolling.
  return (
    <div className="relative flex h-full flex-col bg-[var(--color-canvas)]">
      <div className="flex min-h-0 flex-1 overflow-y-auto px-4">
        <div className="m-auto w-full max-w-md py-8">
          <div className="mb-8 flex flex-col items-center text-center">
            <Logo
              animateIn
              className={cn(
                'mb-4 size-14 rounded-2xl shadow-lg shadow-[var(--color-accent)]/20',
                unlocking && 'oik-logo-pulse',
              )}
            />
            <h1 className="text-2xl font-semibold tracking-tight">
              {isSetup ? t('unlock.titleCreate') : t('unlock.titleWelcome')}
            </h1>
            <p className="mt-2 max-w-sm text-sm text-[var(--color-muted)]">
              {isSetup ? t('unlock.bodyCreate') : t('unlock.bodyWelcome')}
            </p>
          </div>

          <form
            className={cn(
              'rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] p-6 shadow-xl',
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
                  <KeyRound className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-[var(--color-muted)]" />
                  <Input
                    type="password"
                    autoComplete={isSetup ? 'new-password' : 'current-password'}
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    className="pl-10"
                    required
                    autoFocus
                    aria-invalid={error ? true : undefined}
                    aria-describedby={error ? errorId : undefined}
                  />
                </div>
              </Field>

              {isSetup ? (
                <Field label={t('unlock.confirmPassword')}>
                  <Input
                    type="password"
                    autoComplete="new-password"
                    value={confirm}
                    onChange={(e) => setConfirm(e.target.value)}
                    required
                    aria-invalid={error ? true : undefined}
                    aria-describedby={error ? errorId : undefined}
                  />
                </Field>
              ) : null}

              <ErrorBanner id={errorId} message={error} className="text-center" />

              <Button type="submit" busy={busy || unlocking} className="w-full">
                {busy || unlocking
                  ? t('common.working')
                  : isSetup
                    ? t('unlock.createVault')
                    : t('unlock.unlock')}
              </Button>
            </div>
          </form>
        </div>
      </div>

      <div className="flex shrink-0 flex-col items-center gap-2 px-4 pt-2 pb-6">
        <button
          type="button"
          className="h-6 text-[13px] font-medium text-[var(--color-muted)] outline-none hover:text-[var(--color-fg-secondary)] focus-visible:ring-2 focus-visible:ring-[var(--color-accent)]/25"
          onClick={() => void beginUpdateCheck()}
        >
          {t('unlock.update.button')}
        </button>
        <button
          type="button"
          className="h-6 text-[13px] font-medium text-[var(--color-muted)] outline-none hover:text-[var(--color-fg-secondary)] focus-visible:ring-2 focus-visible:ring-[var(--color-accent)]/25"
          onClick={() => void beginRestore()}
          disabled={busy || restoreBusy || restorePicking}
        >
          {t('unlock.restoreFromBackup')}
        </button>
      </div>

      <UnlockUpdateDialog
        state={update}
        onDismiss={dismissUpdate}
        onInstall={() => void installAvailable()}
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

function UnlockUpdateDialog({
  state,
  onDismiss,
  onInstall,
}: {
  state: UpdateUiState
  onDismiss: () => void
  onInstall: () => void
}) {
  const { t } = useI18n()
  const titleId = useId()
  const panelRef = useRef<HTMLDivElement>(null)
  const open = state.kind !== 'idle'
  const canDismiss = state.kind !== 'installing'

  useDialogFocus(panelRef, open, () => {
    if (canDismiss) onDismiss()
  })

  if (state.kind === 'idle') return null

  const copy = dialogCopy(state, t)

  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center bg-[rgba(0,0,0,0.5)] px-4">
      <div
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="w-full max-w-md rounded-[1rem] border border-[var(--color-border)] bg-[var(--color-surface)] p-6 outline-none"
      >
        <h2
          id={titleId}
          className="text-base leading-6 text-[var(--color-fg)]"
          style={{ fontWeight: 560 }}
        >
          {copy.title}
        </h2>
        <p className="mt-2 text-sm leading-5 font-normal text-[var(--color-muted)]">{copy.body}</p>
        {state.kind === 'available' ? (
          <p className="mt-4 text-[13px] leading-5 font-normal text-[#8f9a93]">
            {t('unlock.update.available.honesty')}
          </p>
        ) : null}
        {state.kind === 'installing' ? (
          <div
            className="mt-6 h-1 overflow-hidden rounded-[2px] bg-[#232c26]"
            role="progressbar"
            aria-label={copy.title}
          >
            <div className="h-full w-[30%] rounded-[2px] bg-[var(--color-accent)]" />
          </div>
        ) : null}
        {copy.actions.length > 0 ? (
          <div className="mt-6 flex justify-end gap-2">
            {copy.actions.map((action) =>
              action.kind === 'primary' ? (
                <button
                  key={action.label}
                  type="button"
                  className="h-10 rounded-lg bg-[#35b06b] px-4 text-sm font-medium text-[#0a0e0b] outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-accent)]/25"
                  onClick={onInstall}
                >
                  {action.label}
                </button>
              ) : (
                <button
                  key={action.label}
                  type="button"
                  className="h-10 rounded-lg border border-[#344038] bg-[var(--color-surface-2)] px-4 text-sm font-medium text-[var(--color-fg-secondary)] outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-accent)]/25"
                  onClick={onDismiss}
                >
                  {action.label}
                </button>
              ),
            )}
          </div>
        ) : null}
      </div>
    </div>
  )
}

type DialogAction = { kind: 'secondary'; label: string } | { kind: 'primary'; label: string }

function dialogCopy(
  state: Exclude<UpdateUiState, { kind: 'idle' }>,
  t: (key: string, vars?: Record<string, string | number>) => string,
): { title: string; body: string; actions: DialogAction[] } {
  switch (state.kind) {
    case 'checking':
      return {
        title: t('unlock.update.checking.title'),
        body: t('unlock.update.checking.body'),
        actions: [{ kind: 'secondary', label: t('unlock.update.cancel') }],
      }
    case 'upToDate':
      return {
        title: t('unlock.update.upToDate.title'),
        body: t('unlock.update.upToDate.body'),
        actions: [{ kind: 'secondary', label: t('unlock.update.close') }],
      }
    case 'available':
      return {
        title: t('unlock.update.available.title'),
        body: t('unlock.update.available.version', { version: state.version }),
        actions: [
          { kind: 'secondary', label: t('unlock.update.cancel') },
          { kind: 'primary', label: t('unlock.update.available.confirm') },
        ],
      }
    case 'failed':
      return {
        title: t('unlock.update.failed.title'),
        body: t('unlock.update.failed.body'),
        actions: [{ kind: 'secondary', label: t('unlock.update.close') }],
      }
    case 'installing':
      return {
        title: t('unlock.update.installing.title'),
        body: t('unlock.update.installing.body'),
        actions: [],
      }
  }
}
