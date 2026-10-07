import { useEffect, useMemo, useState } from 'react'
import { Modal } from './Modal'
import { Button, Field, Select } from './ui'
import { cn } from '../lib/cn'
import { formatDate, formatMoney, type Account, type Currency, type CsvImportPreview, type SimpleEntryInput } from '../lib/api'
import {
  applyBulkAccounts,
  defaultChecked,
  postLabel,
  previewSubtitle,
  rowSelectable,
} from '../lib/csvImport'
import { useI18n } from '../lib/I18nProvider'
import { renderUiText } from '../lib/uiText'

type DraftRow = CsvImportPreview['rows'][number] & { checked: boolean }

type Props = {
  open: boolean
  preview: CsvImportPreview | null
  currency: Currency
  walletAccounts: Account[]
  expenseAccounts: Account[]
  incomeAccounts: Account[]
  busy?: boolean
  onClose: () => void
  onConfirm: (input: { rows: SimpleEntryInput[]; include_duplicates: boolean }) => void
}

function kindLabelKey(kind: SimpleEntryInput['kind']): string {
  if (kind === 'income') return 'tx.csv.kind.income'
  if (kind === 'bill') return 'tx.csv.kind.bill'
  if (kind === 'transfer') return 'tx.csv.kind.transfer'
  return 'tx.csv.kind.expense'
}

/** Step 3 of CSV import: review, bulk-edit, then post selected rows. */
export function CsvPreviewModal({
  open,
  preview,
  currency,
  walletAccounts,
  expenseAccounts,
  incomeAccounts,
  busy = false,
  onClose,
  onConfirm,
}: Props) {
  const { t } = useI18n()
  const [drafts, setDrafts] = useState<DraftRow[]>([])
  const [bulkWallet, setBulkWallet] = useState('')
  const [bulkCategory, setBulkCategory] = useState('')

  useEffect(() => {
    if (!open || !preview) return
    setDrafts(
      preview.rows.map((row) => ({
        ...row,
        suggested: row.suggested ? { ...row.suggested } : null,
        checked: defaultChecked(row),
      })),
    )
    setBulkWallet(walletAccounts[0]?.id ?? '')
    setBulkCategory(expenseAccounts[0]?.id ?? incomeAccounts[0]?.id ?? '')
  }, [open, preview, walletAccounts, expenseAccounts, incomeAccounts])

  const categoryAccounts = useMemo(() => {
    const catType = [...expenseAccounts, ...incomeAccounts].find((a) => a.id === bulkCategory)
      ?.account_type
    return { list: [...expenseAccounts, ...incomeAccounts], type: catType ?? null }
  }, [bulkCategory, expenseAccounts, incomeAccounts])

  const selected = drafts.filter((row) => row.checked && row.suggested)
  const duplicateCount = preview?.rows.filter((row) => row.duplicate).length ?? 0
  const selectable = drafts.filter(rowSelectable)
  const allSelectableChecked = selectable.length > 0 && selectable.every((row) => row.checked)

  function toggleRow(sourceRow: number, checked: boolean) {
    setDrafts((prev) =>
      prev.map((row) => {
        if (row.source_row !== sourceRow || !rowSelectable(row)) return row
        return { ...row, checked }
      }),
    )
  }

  function toggleAll(checked: boolean) {
    setDrafts((prev) =>
      prev.map((row) => (rowSelectable(row) ? { ...row, checked } : row)),
    )
  }

  function applyToSelected() {
    setDrafts((prev) =>
      prev.map((row) => {
        if (!row.checked || !row.suggested) return row
        return {
          ...row,
          suggested: applyBulkAccounts(row.suggested, {
            walletId: bulkWallet,
            categoryId: bulkCategory,
            categoryType: categoryAccounts.type,
          }),
        }
      }),
    )
  }

  function confirm() {
    const rows = selected.map((row) => row.suggested).filter((s): s is SimpleEntryInput => s != null)
    if (rows.length === 0) return
    onConfirm({
      rows,
      include_duplicates: selected.some((row) => row.duplicate),
    })
  }

  return (
    <Modal
      open={open}
      title={t('tx.csv.previewTitle')}
      description={preview ? previewSubtitle(preview.rows.length, duplicateCount) : undefined}
      maxWidth="max-w-4xl"
      onClose={onClose}
    >
      <div className="mb-4 flex flex-wrap items-end gap-3">
        <Field label={t('tx.csv.wallet')} className="min-w-[12rem] flex-1">
          <Select
            value={bulkWallet}
            onChange={(e) => setBulkWallet(e.target.value)}
            aria-label={t('tx.csv.bulkWallet')}
          >
            {walletAccounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name} · {currency.code}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={t('tx.csv.category')} className="min-w-[12rem] flex-1">
          <Select
            value={bulkCategory}
            onChange={(e) => setBulkCategory(e.target.value)}
            aria-label={t('tx.csv.bulkCategory')}
          >
            {expenseAccounts.length > 0 ? (
              <optgroup label={t('tx.csv.kind.expense')}>
                {expenseAccounts.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </optgroup>
            ) : null}
            {incomeAccounts.length > 0 ? (
              <optgroup label={t('tx.csv.kind.income')}>
                {incomeAccounts.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </optgroup>
            ) : null}
          </Select>
        </Field>
        <Button
          type="button"
          variant="secondary"
          disabled={selected.length === 0 || busy}
          onClick={applyToSelected}
        >
          {t('tx.csv.applySelected')}
        </Button>
      </div>

      <div className="overflow-x-auto rounded-xl border border-[var(--color-border)]">
        <table className="w-full min-w-[40rem] text-left text-sm">
          <thead>
            <tr className="border-b border-[var(--color-border)] font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
              <th className="w-10 px-3 py-2.5">
                <input
                  type="checkbox"
                  className="size-4 accent-[var(--color-accent)]"
                  checked={allSelectableChecked}
                  disabled={selectable.length === 0 || busy}
                  onChange={(e) => toggleAll(e.target.checked)}
                  aria-label={t('tx.csv.selectAll')}
                />
              </th>
              <th className="px-3 py-2.5">{t('tx.csv.col.date')}</th>
              <th className="px-3 py-2.5">{t('tx.csv.col.description')}</th>
              <th className="px-3 py-2.5 text-right">{t('tx.csv.col.amount')}</th>
              <th className="px-3 py-2.5">{t('tx.csv.col.type')}</th>
              <th className="px-3 py-2.5">{t('tx.csv.col.note')}</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[var(--color-border)]">
            {drafts.map((row) => {
              const selectableRow = rowSelectable(row)
              const kind = row.suggested?.kind
              const income = kind === 'income'
              const amountMinor = row.suggested?.amount_minor ?? null
              return (
                <tr
                  key={row.source_row}
                  className={cn(!selectableRow && 'opacity-60')}
                >
                  <td className="px-3 py-2.5">
                    <input
                      type="checkbox"
                      className="size-4 accent-[var(--color-accent)]"
                      checked={row.checked}
                      disabled={!selectableRow || busy}
                      onChange={(e) => toggleRow(row.source_row, e.target.checked)}
                      aria-label={t('tx.csv.selectRow', { n: row.source_row })}
                    />
                  </td>
                  <td className="px-3 py-2.5 tabular-nums text-[var(--color-fg-secondary)]">
                    {row.suggested ? formatDate(row.suggested.entry_date) : '—'}
                  </td>
                  <td className="max-w-[16rem] truncate px-3 py-2.5 text-[var(--color-fg)]">
                    {row.suggested?.description || '—'}
                  </td>
                  <td
                    className={cn(
                      'px-3 py-2.5 text-right font-medium tabular-nums',
                      income ? 'text-[var(--color-money-in-text)]' : 'text-[var(--color-fg)]',
                    )}
                  >
                    {amountMinor == null
                      ? '—'
                      : formatMoney(amountMinor, currency, undefined, { signed: income })}
                  </td>
                  <td className="px-3 py-2.5">
                    {kind ? (
                      <span
                        className={cn(
                          'inline-flex rounded-full px-2 py-0.5 text-xs font-medium',
                          income
                            ? 'bg-[var(--color-money-in-soft)] text-[var(--color-money-in-text)]'
                            : 'bg-[var(--color-surface-elevated)] text-[var(--color-fg-secondary)]',
                        )}
                      >
                        {t(kindLabelKey(kind))}
                      </span>
                    ) : (
                      '—'
                    )}
                  </td>
                  <td className="px-3 py-2.5 text-xs">
                    {row.error ? (
                      <span className="text-[var(--color-danger)]">
                        {renderUiText(row.error, currency) || t('tx.csv.rowProblem.unreadable')}
                      </span>
                    ) : row.duplicate ? (
                      <span className="text-[var(--color-warning)]">{t('tx.csv.duplicate')}</span>
                    ) : null}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>

      <div className="mt-5 flex justify-end gap-2 border-t border-[var(--color-border)] pt-4">
        <Button type="button" variant="secondary" disabled={busy} onClick={onClose}>
          {t('common.cancel')}
        </Button>
        <Button
          type="button"
          disabled={selected.length === 0}
          busy={busy}
          onClick={confirm}
        >
          {postLabel(selected.length)}
        </Button>
      </div>
    </Modal>
  )
}
