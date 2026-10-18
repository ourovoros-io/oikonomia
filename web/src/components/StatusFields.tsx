import type { ReactNode } from 'react'
import { useI18n } from '../lib/I18nProvider'
import { isBillStatus, shownAccountFields, type BillStatus } from '../lib/simpleEntry'
import { AccountSelect, type AccountField } from './AccountSelect'
import { Field, Select } from './ui'

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
