import type { ReactNode } from 'react'
import type { Account } from '../lib/api'
import { useI18n } from '../lib/I18nProvider'
import { isBillStatus, type BillStatus } from '../lib/simpleEntry'
import { Field, Select } from './ui'

/** One account field of the form: what it holds, how it changes, what it offers. */
export type AccountField = {
  value: string
  onChange: (id: string) => void
  options: readonly Account[]
}

function AccountOptions({ options }: { options: readonly Account[] }) {
  return options.map((a) => (
    <option key={a.id} value={a.id}>
      {a.code} · {a.name}
    </option>
  ))
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
  const noPayable = payable.options.length === 0

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
      {status === 'pay_existing' ? null : (
        <Field label={t('tx.billCategory')}>
          <Select
            value={category.value}
            onChange={(e) => category.onChange(e.target.value)}
            required
          >
            <AccountOptions options={category.options} />
          </Select>
        </Field>
      )}
      {status === 'unpaid' ? null : (
        <Field label={status === 'paid' ? t('tx.paidFrom') : t('tx.payFrom')}>
          <Select value={wallet.value} onChange={(e) => wallet.onChange(e.target.value)} required>
            <AccountOptions options={wallet.options} />
          </Select>
        </Field>
      )}
      {status === 'paid' ? null : (
        <Field label={t('tx.billsPayableAccount')}>
          <Select
            value={payable.value}
            onChange={(e) => payable.onChange(e.target.value)}
            disabled={payableLocked}
            required
          >
            {noPayable ? <option value="">{t('tx.noLiability')}</option> : null}
            <AccountOptions options={payable.options} />
          </Select>
        </Field>
      )}
      {noPayable && status !== 'paid' ? (
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
  const holder = unpaid ? receivable : wallet

  return (
    <>
      <Field label={t('tx.incomeType')}>
        <Select value={category.value} onChange={(e) => category.onChange(e.target.value)} required>
          <AccountOptions options={category.options} />
        </Select>
      </Field>
      <Field label={t('tx.incomeStatus')} className={wide}>
        <Select
          value={unpaid ? 'unpaid' : 'received'}
          onChange={(e) => onUnpaid(e.target.value === 'unpaid')}
        >
          <option value="received">{t('tx.incomeReceivedNow')}</option>
          <option value="unpaid">{t('tx.incomeUnpaid')}</option>
        </Select>
      </Field>
      <Field label={unpaid ? t('tx.receivableAccount') : t('tx.receivedInto')}>
        <Select value={holder.value} onChange={(e) => holder.onChange(e.target.value)} required>
          <AccountOptions options={holder.options} />
        </Select>
      </Field>
    </>
  )
}
