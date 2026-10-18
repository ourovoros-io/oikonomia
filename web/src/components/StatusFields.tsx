import type { ReactNode } from 'react'
import type { Account } from '../lib/api'
import { useI18n } from '../lib/I18nProvider'
import { isBillStatus, shownAccountFields, type BillStatus } from '../lib/simpleEntry'
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

/**
 * The fields of a bill, shared by New entry and the Recurring form: its
 * status, then the accounts that status posts to. Which accounts a status
 * posts to is core's rule; this only shows the fields core reads for it.
 *
 * `payment` is drawn under the status when it is a payment of an existing
 * bill: New entry's bill picker, or the Recurring form's note on which bill
 * each post settles. `wide` is the class that spans the form's row.
 */
export function BillStatusFields({
  status,
  onStatus,
  statusDescribedBy,
  category,
  wallet,
  payable,
  payableLocked = false,
  payment,
  wide,
}: {
  status: BillStatus
  onStatus: (next: BillStatus) => void
  statusDescribedBy?: string | undefined
  category: AccountField
  wallet: AccountField
  payable: AccountField
  payableLocked?: boolean
  payment: ReactNode
  wide: string
}) {
  const { t } = useI18n()
  const shown = shownAccountFields('bill', status, false)
  const noPayable = !payable.options.some((a) => a.is_active)

  return (
    <>
      <Field label={t('tx.billStatus')} className={wide}>
        <Select
          value={status}
          aria-describedby={statusDescribedBy}
          onChange={(e) => {
            if (isBillStatus(e.target.value)) onStatus(e.target.value)
          }}
        >
          <option value="paid">{t('tx.billPaidNow')}</option>
          <option value="unpaid">{t('tx.billUnpaid')}</option>
          <option value="pay_existing">{t('tx.billPayExisting')}</option>
        </Select>
      </Field>
      {status === 'pay_existing' ? payment : null}
      {shown.includes('category') ? (
        <AccountSelect label={t('tx.billCategory')} field={category} />
      ) : null}
      {shown.includes('wallet') ? (
        <AccountSelect
          label={status === 'paid' ? t('tx.paidFrom') : t('tx.payFrom')}
          field={wallet}
        />
      ) : null}
      {shown.includes('payable') ? (
        <AccountSelect
          label={t('tx.billsPayableAccount')}
          field={payable}
          placeholder={noPayable ? t('tx.noLiability') : undefined}
          disabled={payableLocked}
        />
      ) : null}
      {noPayable && shown.includes('payable') ? (
        <p className={`text-xs text-[var(--color-muted)] ${wide}`}>{t('tx.billsPayableTip')}</p>
      ) : null}
    </>
  )
}

/**
 * The fields of an income, shared by New entry and the Recurring form: its
 * category, whether it is received or still owed, and the account that
 * holds it, the receivable when owed.
 */
export function IncomeStatusFields({
  unpaid,
  onUnpaid,
  category,
  receivable,
  wallet,
  wide,
}: {
  unpaid: boolean
  onUnpaid: (unpaid: boolean) => void
  category: AccountField
  receivable: AccountField
  wallet: AccountField
  wide: string
}) {
  const { t } = useI18n()

  return (
    <>
      <AccountSelect label={t('tx.incomeType')} field={category} />
      <Field label={t('tx.incomeStatus')} className={wide}>
        <Select
          value={unpaid ? 'unpaid' : 'received'}
          onChange={(e) => onUnpaid(e.target.value === 'unpaid')}
        >
          <option value="received">{t('tx.incomeReceivedNow')}</option>
          <option value="unpaid">{t('tx.incomeUnpaid')}</option>
        </Select>
      </Field>
      <AccountSelect
        label={unpaid ? t('tx.receivableAccount') : t('tx.receivedInto')}
        field={unpaid ? receivable : wallet}
      />
    </>
  )
}
