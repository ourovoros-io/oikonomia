import type { Account } from '../lib/api'
import { useI18n } from '../lib/I18nProvider'
import { Field, Select } from './ui'

/**
 * One account field of the form: what it holds, how it changes, what it
 * offers. `options` are the active accounts it accepts, plus the held account
 * when that one is archived (`accountChoices`).
 */
export type AccountField = {
  value: string
  onChange: (id: string) => void
  options: readonly Account[]
}

/**
 * An account select that shows the account the form holds. When that is no
 * active option (none picked, archived, or not yet loaded), the browser would
 * show the first account while the form keeps another, so the held value gets
 * its own option: the archived account marked as such, else `placeholder`.
 */
export function AccountSelect({
  label,
  field,
  placeholder,
  disabled = false,
}: {
  label: string
  field: AccountField
  placeholder?: string | undefined
  disabled?: boolean
}) {
  const { t } = useI18n()
  const held = field.options.find((a) => a.id === field.value)

  return (
    <Field label={label}>
      <Select
        value={field.value}
        onChange={(e) => field.onChange(e.target.value)}
        disabled={disabled}
        required
        aria-invalid={held?.is_active ? undefined : true}
      >
        {held ? null : (
          <option value={field.value} disabled>
            {placeholder ?? t('tx.accountChoose')}
          </option>
        )}
        {field.options.map((a) => (
          <option key={a.id} value={a.id} disabled={!a.is_active}>
            {a.is_active
              ? `${a.code} · ${a.name}`
              : t('tx.accountArchived', { code: a.code, name: a.name })}
          </option>
        ))}
      </Select>
    </Field>
  )
}
