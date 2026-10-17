import { useEffect, useState } from 'react'
import { api, formatDate, formatMoney, type OpenBill } from '../lib/api'
import { commandErrorMessage } from '../lib/commandError'
import { useI18n } from '../lib/I18nProvider'
import type { Currency } from '../lib/money'
import { Field, Select } from './ui'

type Props = {
  entityId: string
  /** The payment being corrected, whose bill is offered again; null for a new payment. */
  paymentId: string | null
  /** The picked bill's id, or '' for a payment of no bill in particular. */
  value: string
  currency: Currency
  /** The picked bill, or null when the user picks no bill. */
  onPick: (bill: OpenBill | null) => void
  onError: (message: string) => void
}

/**
 * The open bills of a book, for "Pay an existing unpaid bill". Picking one
 * hands it to the form, which fills in its amount and payable account; core
 * checks that the payment matches the bill it settles.
 */
export function BillPicker({ entityId, paymentId, value, currency, onPick, onError }: Props) {
  const { t } = useI18n()
  const [bills, setBills] = useState<OpenBill[]>([])

  useEffect(() => {
    let current = true
    api.billListOpen(entityId, paymentId).then(
      (open) => {
        if (current) setBills(open)
      },
      (err: unknown) => {
        if (current) onError(commandErrorMessage(err))
      },
    )

    return () => {
      current = false
    }
    // onError is a fresh closure each render; the list depends on the book and payment only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entityId, paymentId])

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
