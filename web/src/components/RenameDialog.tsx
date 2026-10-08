import { useState, type FormEvent } from 'react'
import { Modal } from './Modal'
import { Button, Field, Input } from './ui'
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
  const { t } = useI18n()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  function close() {
    setError(null)
    onClose()
  }

  async function submit(name: string) {
    setBusy(true)
    setError(null)
    try {
      await onSave(name)
      close()
    } catch (err) {
      setError(commandErrorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open={current !== null}
      title={title}
      maxWidth="max-w-md"
      error={error}
      onDismissError={() => setError(null)}
      onClose={() => {
        if (!busy) close()
      }}
    >
      {/* Mounted only while the dialog is open, so each opening starts from the current name. */}
      <RenameForm
        current={current ?? ''}
        label={label}
        busy={busy}
        onSubmitName={submit}
        onCancel={close}
        cancelLabel={t('common.cancel')}
        submitLabel={busy ? t('common.saving') : t('common.rename')}
      />
    </Modal>
  )
}

function RenameForm({
  current,
  label,
  busy,
  onSubmitName,
  onCancel,
  cancelLabel,
  submitLabel,
}: {
  current: string
  label: string
  busy: boolean
  onSubmitName: (name: string) => Promise<void>
  onCancel: () => void
  cancelLabel: string
  submitLabel: string
}) {
  const [name, setName] = useState(current)

  function onSubmit(ev: FormEvent) {
    ev.preventDefault()
    void onSubmitName(name)
  }

  return (
    <form noValidate onSubmit={onSubmit} className="space-y-4">
      <Field label={label}>
        <Input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </Field>
      <div className="flex justify-end gap-2 border-t border-[var(--color-border)] pt-4">
        <Button type="button" variant="secondary" disabled={busy} onClick={onCancel}>
          {cancelLabel}
        </Button>
        <Button type="submit" busy={busy} disabled={name.trim() === current.trim()}>
          {submitLabel}
        </Button>
      </div>
    </form>
  )
}
