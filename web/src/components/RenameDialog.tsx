import { useState, type FormEvent } from 'react'
import { Modal } from './Modal'
import { Button, ErrorBanner, Field, Input } from './ui'
import { commandErrorMessage } from '../lib/commandError'
import { useI18n } from '../lib/I18nProvider'

type Props = {
  /** The name being changed; null keeps the dialog closed. */
  current: string | null
  title: string
  label: string
  /** Saves the new name. A rejection is shown in the dialog and keeps it open. */
  onSave: (name: string) => Promise<void>
  onClose: () => void
}

/** A one-field dialog for renaming a book or an account. */
export function RenameDialog({ current, title, label, onSave, onClose }: Props) {
  const [busy, setBusy] = useState(false)

  return (
    <Modal
      open={current !== null}
      title={title}
      maxWidth="max-w-md"
      onClose={() => {
        if (!busy) onClose()
      }}
    >
      {/* Mounted only while the dialog is open, so each opening starts from the current name. */}
      <RenameForm
        current={current ?? ''}
        label={label}
        busy={busy}
        onBusy={setBusy}
        onSave={onSave}
        onClose={onClose}
      />
    </Modal>
  )
}

function RenameForm({
  current,
  label,
  busy,
  onBusy,
  onSave,
  onClose,
}: {
  current: string
  label: string
  busy: boolean
  onBusy: (busy: boolean) => void
  onSave: (name: string) => Promise<void>
  onClose: () => void
}) {
  const { t } = useI18n()
  const [name, setName] = useState(current)
  const [error, setError] = useState<string | null>(null)

  async function onSubmit(ev: FormEvent) {
    ev.preventDefault()
    onBusy(true)
    setError(null)
    try {
      await onSave(name)
      onClose()
    } catch (err) {
      setError(commandErrorMessage(err))
    } finally {
      onBusy(false)
    }
  }

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <ErrorBanner message={error} className="mb-0" />
      <Field label={label}>
        <Input value={name} onChange={(e) => setName(e.target.value)} required autoFocus />
      </Field>
      <div className="flex justify-end gap-2 border-t border-[var(--color-border)] pt-4">
        <Button type="button" variant="secondary" disabled={busy} onClick={onClose}>
          {t('common.cancel')}
        </Button>
        <Button type="submit" busy={busy} disabled={name.trim() === current.trim()}>
          {busy ? t('common.saving') : t('common.rename')}
        </Button>
      </div>
    </form>
  )
}
