import { useEffect, useState } from 'react'
import { ArrowRight } from 'lucide-react'
import { Modal } from './Modal'
import { Button, Input, Select } from './ui'
import type { CsvColumnMapping, CsvImportPreview } from '../lib/api'
import {
  draftFromDetected,
  draftToMapping,
  draftsEqual,
  mapNeededKey,
  mappingReady,
  matchHeader,
  previewHasColumnMap,
  type CsvMapDraft,
} from '../lib/csvImport'
import { useI18n } from '../lib/I18nProvider'

type Props = {
  open: boolean
  preview: CsvImportPreview | null
  busy?: boolean
  onClose: () => void
  onContinue: (mapping: CsvColumnMapping, unchanged: boolean) => void
}

const PLACEHOLDER_FIELDS = [
  { labelKey: 'tx.csv.field.date', targetKey: 'tx.csv.field.date' },
  { labelKey: 'tx.csv.field.amount', targetKey: 'tx.csv.field.amount' },
  { labelKey: 'tx.csv.field.description', targetKey: 'tx.csv.field.description' },
  { labelKey: 'tx.csv.field.reference', targetKey: 'tx.csv.field.referenceOptional' },
] as const

function HeaderSelect({
  label,
  target,
  value,
  headers,
  optional,
  onChange,
}: {
  label: string
  target: string
  value: string
  headers: string[]
  optional?: boolean
  onChange: (value: string) => void
}) {
  const { t } = useI18n()
  return (
    <div className="flex items-center gap-3">
      <span className="w-28 shrink-0 font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
        {label}
      </span>
      <Select
        className="min-w-0 flex-1"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        aria-label={t('tx.csv.sourceAria', { label })}
      >
        {optional || !value ? <option value="">{t('tx.csv.notMapped')}</option> : null}
        {headers.map((header) => (
          <option key={header} value={header}>
            {header}
          </option>
        ))}
      </Select>
      <ArrowRight className="size-4 shrink-0 text-[var(--color-muted)]" aria-hidden />
      <span className="w-40 shrink-0 text-sm text-[var(--color-fg-secondary)]">{target}</span>
    </div>
  )
}

function PlaceholderRow({ label, target }: { label: string; target: string }) {
  const { t } = useI18n()
  return (
    <div className="flex items-center gap-3">
      <span className="w-28 shrink-0 font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
        {label}
      </span>
      <Input
        value={t('tx.csv.autoDetected')}
        disabled
        readOnly
        aria-label={t('tx.csv.sourceAria', { label })}
      />
      <ArrowRight className="size-4 shrink-0 text-[var(--color-muted)]" aria-hidden />
      <Input
        value={target}
        disabled
        readOnly
        aria-label={t('tx.csv.targetAria', { label })}
      />
    </div>
  )
}

/** Step 2 of CSV import: map file headers, or Auto-detected placeholders until headers exist. */
export function CsvMappingModal({ open, preview, busy = false, onClose, onContinue }: Props) {
  const { t } = useI18n()
  const headers = preview?.headers ?? []
  const live = preview != null && previewHasColumnMap(preview)
  const [draft, setDraft] = useState<CsvMapDraft>(() =>
    draftFromDetected(headers, preview?.detected_mapping ?? {}),
  )

  useEffect(() => {
    if (!open || !preview) return
    setDraft(draftFromDetected(preview.headers ?? [], preview.detected_mapping ?? {}))
  }, [open, preview])

  const mapping = draftToMapping(draft)
  const canContinue = !live || mappingReady(mapping)
  const neededKey = live ? mapNeededKey(preview?.missing_columns) : null

  function patch(partial: Partial<CsvMapDraft>) {
    setDraft((prev) => ({ ...prev, ...partial }))
  }

  function toggleAmountMode() {
    setDraft((prev) => {
      if (prev.amountMode === 'amount') {
        return {
          ...prev,
          amountMode: 'debit_credit',
          amount: '',
          debit: matchHeader(headers, preview?.detected_mapping?.debit) || prev.debit,
          credit: matchHeader(headers, preview?.detected_mapping?.credit) || prev.credit,
        }
      }
      return {
        ...prev,
        amountMode: 'amount',
        debit: '',
        credit: '',
        amount: matchHeader(headers, preview?.detected_mapping?.amount) || prev.amount,
      }
    })
  }

  function handleContinue() {
    if (!canContinue || !preview || busy) return
    if (!live) {
      onContinue({}, true)
      return
    }
    const initial = draftFromDetected(preview.headers ?? [], preview.detected_mapping ?? {})
    onContinue(mapping, draftsEqual(draft, initial))
  }

  return (
    <Modal
      open={open}
      title={t('tx.csv.mapTitle')}
      description={t('tx.csv.mapDescription')}
      maxWidth="max-w-xl"
      onClose={onClose}
    >
      {neededKey ? (
        <p role="status" className="mb-4 text-sm text-[var(--color-fg)]">
          {t(neededKey)}
        </p>
      ) : null}
      <div className="space-y-3">
        {live ? (
          <>
            <HeaderSelect
              label={t('tx.csv.field.date')}
              target={t('tx.csv.field.date')}
              value={draft.date}
              headers={headers}
              onChange={(date) => patch({ date })}
            />
            {draft.amountMode === 'debit_credit' ? (
              <>
                <HeaderSelect
                  label={t('tx.csv.field.debit')}
                  target={t('tx.csv.field.debit')}
                  value={draft.debit}
                  headers={headers}
                  onChange={(debit) => patch({ debit })}
                />
                <HeaderSelect
                  label={t('tx.csv.field.credit')}
                  target={t('tx.csv.field.credit')}
                  value={draft.credit}
                  headers={headers}
                  onChange={(credit) => patch({ credit })}
                />
              </>
            ) : (
              <>
                <HeaderSelect
                  label={t('tx.csv.field.amount')}
                  target={t('tx.csv.field.amount')}
                  value={draft.amount}
                  headers={headers}
                  onChange={(amount) => patch({ amount })}
                />
                <HeaderSelect
                  label={t('tx.csv.field.direction')}
                  target={t('tx.csv.field.directionOptional')}
                  value={draft.direction}
                  headers={headers}
                  optional
                  onChange={(direction) => patch({ direction })}
                />
              </>
            )}
            <HeaderSelect
              label={t('tx.csv.field.description')}
              target={t('tx.csv.field.description')}
              value={draft.description}
              headers={headers}
              onChange={(description) => patch({ description })}
            />
            <HeaderSelect
              label={t('tx.csv.field.reference')}
              target={t('tx.csv.field.referenceOptional')}
              value={draft.reference}
              headers={headers}
              optional
              onChange={(reference) => patch({ reference })}
            />
          </>
        ) : (
          PLACEHOLDER_FIELDS.map((row) => (
            <PlaceholderRow
              key={row.labelKey}
              label={t(row.labelKey)}
              target={t(row.targetKey)}
            />
          ))
        )}
      </div>

      {live ? (
        <button
          type="button"
          className="mt-3 text-xs text-[var(--color-muted)] underline-offset-2 hover:text-[var(--color-fg)] hover:underline"
          onClick={toggleAmountMode}
        >
          {draft.amountMode === 'amount' ? t('tx.csv.useDebitCredit') : t('tx.csv.useAmount')}
        </button>
      ) : null}

      <p className="mt-5 text-xs text-[var(--color-muted)]">{t('tx.csv.mapFooter')}</p>

      <div className="mt-5 flex justify-end gap-2 border-t border-[var(--color-border)] pt-4">
        <Button type="button" variant="secondary" disabled={busy} onClick={onClose}>
          {t('common.cancel')}
        </Button>
        <Button type="button" disabled={!canContinue} busy={busy} onClick={handleContinue}>
          {t('tx.csv.continuePreview')}
        </Button>
      </div>
    </Modal>
  )
}
