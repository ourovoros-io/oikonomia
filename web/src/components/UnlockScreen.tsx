import { useState, type FormEvent } from 'react'
import { KeyRound } from 'lucide-react'
import { Logo } from './Logo'
import { ConfirmDialog } from './ConfirmDialog'
import type { VaultStatus } from '../lib/tauri'
import { vaultInit, vaultRestore, vaultStatus, vaultUnlock, pickVaultBackup, type CommandError } from '../lib/tauri'
import { backupCommandError, restoreArgs, restoreConfirm } from '../lib/vaultBackupUi'
import { Button, ErrorBanner, Field, Input } from './ui'

type Props = {
  status: Exclude<VaultStatus, 'unlocked'>
  onUnlocked: (status: VaultStatus) => void
}

export function UnlockScreen({ status, onUnlocked }: Props) {
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [restoreOpen, setRestoreOpen] = useState(false)
  const [restoreBusy, setRestoreBusy] = useState(false)
  const [restorePath, setRestorePath] = useState<string | undefined>(undefined)
  const [restorePicking, setRestorePicking] = useState(false)

  const isSetup = status === 'uninitialized'
  const restorePrompt = restoreConfirm(isSetup ? 'load' : 'replace')

  async function onSubmit(event: FormEvent) {
    event.preventDefault()
    setError(null)

    if (isSetup && password !== confirm) {
      setError('Passwords do not match')
      return
    }

    // Password strength rules live in Rust; its Validation error surfaces below.
    setBusy(true)
    try {
      const next = isSetup ? await vaultInit(password) : await vaultUnlock(password)
      onUnlocked(next)
    } catch (err) {
      const cmd = err as CommandError
      setError(
        cmd.code === 'invalid_password'
          ? 'Incorrect password — please try again.'
          : cmd.message || 'Could not unlock the vault.',
      )
    } finally {
      setBusy(false)
      setPassword('')
      setConfirm('')
    }
  }

  async function beginRestore() {
    if (busy || restoreBusy || restorePicking || restoreOpen) return
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
      const result = await vaultRestore(restoreArgs(restorePath, restorePrompt.replace))
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

  // m-auto instead of items-center: when the window is shorter than the
  // form, auto margins collapse and the top stays reachable by scrolling.
  return (
    <div className="flex h-full flex-col bg-[var(--color-canvas)]">
      <div className="flex min-h-0 flex-1 overflow-y-auto px-4">
        <div className="m-auto w-full max-w-md py-8">
          <div className="mb-8 flex flex-col items-center text-center">
            <Logo className="mb-4 size-14 rounded-2xl shadow-lg shadow-[var(--color-accent)]/20" />
            <h1 className="text-2xl font-semibold tracking-tight">
              {isSetup ? 'Create your vault' : 'Welcome back'}
            </h1>
            <p className="mt-2 max-w-sm text-sm text-[var(--color-muted)]">
              {isSetup
                ? 'Choose a password. It is never stored. If you lose it, the books cannot be recovered.'
                : 'Enter your password to decrypt this device’s books.'}
            </p>
          </div>

          <form
            className="rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] p-6 shadow-xl"
            onSubmit={onSubmit}
          >
            <div className="space-y-4">
              <Field label="Password">
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
                  />
                </div>
              </Field>

              {isSetup ? (
                <Field label="Confirm password">
                  <Input
                    type="password"
                    autoComplete="new-password"
                    value={confirm}
                    onChange={(e) => setConfirm(e.target.value)}
                    required
                  />
                </Field>
              ) : null}

              <ErrorBanner message={error} className="text-center" />

              <Button type="submit" busy={busy} className="w-full">
                {busy ? 'Working…' : isSetup ? 'Create encrypted vault' : 'Unlock'}
              </Button>
            </div>
          </form>
        </div>
      </div>

      <div className="shrink-0 px-4 pb-6 pt-2 text-center">
        <button
          type="button"
          className="text-xs text-[var(--color-muted)] transition hover:text-[var(--color-fg-secondary)]"
          onClick={() => void beginRestore()}
          disabled={busy || restoreBusy || restorePicking}
        >
          Restore from backup
        </button>
      </div>

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
