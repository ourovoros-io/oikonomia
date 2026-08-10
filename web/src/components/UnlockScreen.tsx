import { useState, type FormEvent } from 'react'
import { Building2, KeyRound, Shield } from 'lucide-react'
import type { VaultStatus } from '../lib/tauri'
import { vaultInit, vaultUnlock, type CommandError } from '../lib/tauri'
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

  const isSetup = status === 'uninitialized'

  async function onSubmit(event: FormEvent) {
    event.preventDefault()
    setError(null)

    if (isSetup && password !== confirm) {
      setError('Passwords do not match')
      return
    }

    if (password.length < 12) {
      setError('Use at least 12 characters')
      return
    }

    setBusy(true)
    try {
      const next = isSetup ? await vaultInit(password) : await vaultUnlock(password)
      onUnlocked(next)
    } catch (err) {
      const cmd = err as CommandError
      setError(cmd.message || 'Could not unlock vault')
    } finally {
      setBusy(false)
      setPassword('')
      setConfirm('')
    }
  }

  return (
    <div className="flex h-full items-center justify-center bg-[var(--color-canvas)] px-4">
      <div className="w-full max-w-md">
        <div className="mb-8 flex flex-col items-center text-center">
          <span className="mb-4 flex size-12 items-center justify-center rounded-2xl bg-[var(--color-accent)] text-white shadow-lg shadow-[var(--color-accent)]/20">
            <Building2 className="size-6" strokeWidth={1.75} />
          </span>
          <h1 className="text-2xl font-semibold tracking-tight">
            {isSetup ? 'Create your vault' : 'Welcome back'}
          </h1>
          <p className="mt-2 max-w-sm text-sm text-[var(--color-muted)]">
            {isSetup
              ? 'Choose a master password. It is never stored. If you lose it, the books cannot be recovered.'
              : 'Enter your master password to decrypt this device’s books.'}
          </p>
        </div>

        <form
          className="rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] p-6 shadow-xl"
          onSubmit={onSubmit}
        >
          <div className="mb-5 flex items-center gap-2 text-xs text-[var(--color-muted)]">
            <Shield className="size-3.5 text-[var(--color-accent)]" strokeWidth={1.75} />
            Encrypted at rest with SQLCipher
          </div>

          <div className="space-y-4">
            <Field label="Master password">
              <div className="relative">
                <KeyRound className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-[var(--color-muted)]" />
                <Input
                  type="password"
                  autoComplete={isSetup ? 'new-password' : 'current-password'}
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className="pl-10"
                  required
                  minLength={12}
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
                  minLength={12}
                />
              </Field>
            ) : null}

            <ErrorBanner message={error} />

            <Button type="submit" disabled={busy} className="w-full">
              {busy ? 'Working…' : isSetup ? 'Create encrypted vault' : 'Unlock'}
            </Button>
          </div>
        </form>
      </div>
    </div>
  )
}
