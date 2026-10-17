import { useEffect, useState } from 'react'
import { api, formatDate, formatMoney, type OpenBill } from '../lib/api'
import { commandFailure, type UiError } from '../lib/uiError'
import { useI18n } from '../lib/I18nProvider'
import type { Currency } from '../lib/money'
import { Field, Select } from './ui'

type Props = {
  entityId: string
  /** The payment being corrected, whose bill is offered again; null for a new payment. */
  correctingPaymentId: string | null
  /** The picked bill's id, or '' for a payment of no bill in particular. */
  value: string
  currency: Currency
  /** The picked bill, or null when the user picks no bill. */
  onPick: (bill: OpenBill | null) => void
  onError: (error: UiError) => void
}

/**
 * The open bills of a book, for "Pay an existing unpaid bill". Picking one
 * hands it to the form, which fills in its amount and payable account; core
 * checks that the payment matches the bill it settles.
 */
export function BillPicker({ entityId, correctingPaymentId, value, currency, onPick, onError }: Props) {
  const { t } = useI18n()
  const [bills, setBills] = useState<OpenBill[]>([])

  useEffect(() => {
    let current = true
    api.billListOpen(entityId, correctingPaymentId).then(
      (open) => {
        if (current) setBills(open)
      },
      (err: unknown) => {
        if (current) onError(commandFailure(err))
      },
    )

    return () => {
      current = false
    }
  }, [entityId, correctingPaymentId, onError])

  return (
    <Field label={t('tx.billToPay')} className="sm:col-span-2 lg:col-span-3">
      <Select
        value={value}
        onChange={(e) => onPick(bills.find((bill) => bill.entry_id === e.target.value) ?? null)}
      >
        <option value="">{t('tx.billToPayNone')}</option>
        {bills.map((bill) => (
          <option key={bill.entry_id} value={bill.entry_id}>
            {formatDate(bill.entry_date)} · {bill.description} ·{' '}
            {formatMoney(bill.amount_minor, currency)}
          </option>
        ))}
      </Select>
    </Field>
  )
}
