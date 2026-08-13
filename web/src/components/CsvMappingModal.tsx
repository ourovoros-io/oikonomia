import { useEffect, useState } from 'react'
import { ArrowRight } from 'lucide-react'
import { Modal } from './Modal'
import { Button, Input, Select } from './ui'
import type { CsvColumnMapping, CsvImportPreview } from '../lib/api'
import {
  CSV_MAP_FOOTER_NOTE,
  CSV_MAP_SOURCE_FALLBACK,
  draftFromDetected,
  draftToMapping,
  draftsEqual,
  mappingReady,
  matchHeader,
  previewHasColumnMap,
  type CsvMapDraft,
} from '../lib/csvImport'

type Props = {
  open: boolean
  preview: CsvImportPreview | null
  busy?: boolean
  onClose: () => void
  onContinue: (mapping: CsvColumnMapping, unchanged: boolean) => void
}

const PLACEHOLDER_ROWS = [
  { label: 'Date', target: 'Date' },
  { label: 'Amount', target: 'Amount' },
  { label: 'Description', target: 'Description' },
  { label: 'Reference', target: 'Reference (optional)' },
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
  return (
    <div className="flex items-center gap-3">
      <span className="w-28 shrink-0 text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
        {label}
      </span>
      <Select
        className="min-w-0 flex-1"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        aria-label={`${label} source column`}
      >
        {optional || !value ? <option value="">Not mapped</option> : null}
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
  return (
    <div className="flex items-center gap-3">
      <span className="w-28 shrink-0 text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
        {label}
      </span>
      <Input
        value={CSV_MAP_SOURCE_FALLBACK}
        disabled
        readOnly
        aria-label={`${label} source column`}
      />
      <ArrowRight className="size-4 shrink-0 text-[var(--color-muted)]" aria-hidden />
      <Input value={target} disabled readOnly aria-label={`${label} target field`} />
    </div>
  )
}

/** Step 2 of CSV import: map file headers, or Auto-detected placeholders until headers exist. */
export function CsvMappingModal({ open, preview, busy = false, onClose, onContinue }: Props) {
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
      title="Map CSV columns"
      description="Match your bank file to a date, an amount (or debit/credit), and a description. Reference is optional."
      maxWidth="max-w-xl"
      onClose={onClose}
    >
      <div className="space-y-3">
        {live ? (
          <>
            <HeaderSelect
              label="Date"
              target="Date"
              value={draft.date}
              headers={headers}
              onChange={(date) => patch({ date })}
            />
            {draft.amountMode === 'debit_credit' ? (
              <>
                <HeaderSelect
                  label="Debit"
                  target="Debit"
                  value={draft.debit}
                  headers={headers}
                  onChange={(debit) => patch({ debit })}
                />
                <HeaderSelect
                  label="Credit"
                  target="Credit"
                  value={draft.credit}
                  headers={headers}
                  onChange={(credit) => patch({ credit })}
                />
              </>
            ) : (
              <HeaderSelect
                label="Amount"
                target="Amount"
                value={draft.amount}
                headers={headers}
                onChange={(amount) => patch({ amount })}
              />
            )}
            <HeaderSelect
              label="Description"
              target="Description"
              value={draft.description}
              headers={headers}
              onChange={(description) => patch({ description })}
            />
            <HeaderSelect
              label="Reference"
              target="Reference (optional)"
              value={draft.reference}
              headers={headers}
              optional
              onChange={(reference) => patch({ reference })}
            />
          </>
        ) : (
          PLACEHOLDER_ROWS.map((row) => (
            <PlaceholderRow key={row.label} label={row.label} target={row.target} />
          ))
        )}
      </div>

      {live ? (
        <button
          type="button"
          className="mt-3 text-xs text-[var(--color-muted)] underline-offset-2 hover:text-[var(--color-fg)] hover:underline"
          onClick={toggleAmountMode}
        >
          {draft.amountMode === 'amount'
            ? 'Use debit and credit columns'
            : 'Use a single amount column'}
        </button>
      ) : null}

      <p className="mt-5 text-xs text-[var(--color-muted)]">{CSV_MAP_FOOTER_NOTE}</p>

      <div className="mt-5 flex justify-end gap-2 border-t border-[var(--color-border)] pt-4">
        <Button type="button" variant="secondary" disabled={busy} onClick={onClose}>
          Cancel
        </Button>
        <Button type="button" disabled={!canContinue} busy={busy} onClick={handleContinue}>
          Continue to preview
        </Button>
      </div>
    </Modal>
  )
}
