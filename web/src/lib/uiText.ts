import { t } from './i18n'
import { formatDate, formatMoney, type Currency } from './money'

/**
 * Text Rust decides on and the UI words: a stable code and the values to fill
 * in. Rust never sends the sentence (see `ui_text.rs` in oikonomia-core).
 */
export type UiText = {
  code: string
  params?: Record<string, string>
}

/**
 * Coded text -> catalog key: analyzer notes and CSV import row problems. Every code Rust can emit is listed in
 * uiTextCodes.json, and a test fails when the two sides differ.
 */
export const NOTE_CODE_KEYS: Record<string, string> = {
  no_text_extracted: 'analyze.notes.noText',
  amount_assumes_two_decimals: 'analyze.notes.amountCurrency',
  dated_from_document: 'analyze.notes.datedFromDoc',
  add_payable_account: 'analyze.notes.addPayable',
  pdf_over_budget: 'analyze.notes.pdfBudget',
  parsed_from_document_text: 'analyze.notes.parsedText',
  ocr_path_missing: 'analyze.notes.ocrPathMissing',
  ocr_models_missing: 'analyze.notes.ocrModelsMissing',
  ocr_read: 'analyze.notes.ocrRead',
  ocr_little_text: 'analyze.notes.ocrLittleText',
  ocr_failed: 'analyze.notes.ocrError',
  ocr_pdf_image: 'analyze.notes.ocrPdfImage',
  invoice_parsed: 'analyze.invoice.notes.parsed',
  invoice_no_total: 'analyze.invoice.notes.noTotal',
  invoice_income: 'analyze.invoice.notes.income',
  invoice_utility: 'analyze.invoice.notes.utility',
  invoice_unpaid: 'analyze.invoice.notes.unpaid',
  invoice_vat_exempt: 'analyze.invoice.notes.vatExempt',
  transfer_detected: 'analyze.invoice.notes.transferDetected',
  transfer_no_amount: 'analyze.invoice.notes.transferNoAmount',
  transfer_fee: 'analyze.invoice.notes.transferFee',
  transfer_fee_unstated: 'analyze.invoice.notes.transferFeeUnstated',
  csv_invalid_date: 'tx.csv.rowProblem.invalidDate',
  csv_invalid_amount: 'tx.csv.rowProblem.invalidAmount',
  csv_invalid_type: 'tx.csv.rowProblem.invalidType',
  csv_missing_date: 'tx.csv.rowProblem.missingDate',
  csv_missing_amount: 'tx.csv.rowProblem.missingAmount',
  csv_zero_amount: 'tx.csv.rowProblem.zeroAmount',
  csv_amount_overflow: 'tx.csv.rowProblem.amountOverflow',
  csv_unreadable_row: 'tx.csv.rowProblem.unreadable',
}

/** Analyzer status hint code -> catalog key. */
export const HINT_KEYS: Record<string, string> = {
  ready: 'analyze.hint.ready',
  models_missing: 'analyze.hint.missingModels',
}

/** Synthetic report row kind -> catalog key. */
export const SYNTHETIC_LINE_KEYS: Record<string, string> = {
  retained_earnings: 'reports.synthetic.retainedEarnings',
  net_income: 'reports.synthetic.netIncome',
}

/**
 * Money values a note carries as integer minor units: the placeholder the copy
 * uses, and the params that hold the minor units and their currency. Core only
 * sends an amount in the currency of the book the note is about, so the book's
 * decimals format it.
 */
const MONEY_PARAMS: Record<string, { placeholder: string; minor: string; currency: string }> = {
  transfer_fee: { placeholder: 'fee', minor: 'fee_minor', currency: 'currency' },
}

/** Rust sends ISO dates; the copy shows the date the way the rest of the app does. */
const DATE_PARAMS: ReadonlySet<string> = new Set(['date'])

function keyFor(map: Record<string, string>, code: string): string | undefined {
  return Object.hasOwn(map, code) ? map[code] : undefined
}

/** A whole number of minor units, as Rust sends it. Nothing else may become a number. */
const INTEGER_TEXT = /^-?\d+$/

/** Any `{name}` still in the copy once the values are filled in. */
const LEFTOVER_PLACEHOLDER = /\{\w+\}/

/**
 * The text for a note in the current language, or an empty string when the
 * code is unknown or its values are unusable. A code is never shown: an
 * unknown one is a version mismatch, so it is logged and skipped. Copy with a
 * value still missing is skipped too, never shown with a raw `{name}` in it.
 *
 * `book` is the currency of the book the text is about, or null when there is
 * no book. A note that carries money is skipped without it, and when it names
 * another currency: only the book's currency comes with the number of decimals
 * core counts in.
 */
export function renderUiText(text: UiText, book: Currency | null): string {
  const key = keyFor(NOTE_CODE_KEYS, text.code)

  if (key === undefined) {
    console.warn(`Unknown UI text code "${text.code}"`)
    return ''
  }

  const vars: Record<string, string> = {}

  for (const [name, value] of Object.entries(text.params ?? {})) {
    vars[name] = DATE_PARAMS.has(name) ? formatDate(value) : value
  }

  const spec = Object.hasOwn(MONEY_PARAMS, text.code) ? MONEY_PARAMS[text.code] : undefined

  if (spec !== undefined) {
    const rawMinor = text.params?.[spec.minor]
    const currency = text.params?.[spec.currency]

    if (rawMinor === undefined || !INTEGER_TEXT.test(rawMinor) || !currency) {
      console.warn(`UI text "${text.code}" is missing its money values`)
      return ''
    }

    if (book === null || book.code !== currency) {
      console.warn(`UI text "${text.code}" carries money that is not in the book's currency`)
      return ''
    }

    // The same call as the suggested amount beside the fee in the same banner,
    // so the two always look alike.
    vars[spec.placeholder] = formatMoney(Number(rawMinor), book)
  }

  const copy = t(key, vars)

  if (copy === key) return ''

  if (LEFTOVER_PLACEHOLDER.test(copy)) {
    console.warn(`UI text "${text.code}" is missing a value for its copy`)
    return ''
  }

  return copy
}

/** All notes in the current language, in order, joined by a space; unknown codes are skipped. */
export function renderUiTexts(notes: readonly UiText[], book: Currency | null): string {
  return notes
    .map((note) => renderUiText(note, book))
    .filter((sentence) => sentence !== '')
    .join(' ')
}

/** The analyzer status line for a hint code, or an empty string for an unknown one. */
export function renderAnalyzerHint(hint: string): string {
  const key = keyFor(HINT_KEYS, hint)

  if (key === undefined) {
    console.warn(`Unknown analyzer hint "${hint}"`)
    return ''
  }

  return t(key)
}

/** The name to show for a report line: the translated label for a computed row, else its own name. */
export function reportLineName(line: { name: string; synthetic?: string | null }): string {
  return line.synthetic ? syntheticLineLabel(line.synthetic, line.name) : line.name
}

/** The translated label for a synthetic report row, or its English name when the kind is unknown. */
export function syntheticLineLabel(kind: string, fallbackName: string): string {
  const key = keyFor(SYNTHETIC_LINE_KEYS, kind)

  if (key === undefined) {
    console.warn(`Unknown synthetic report line "${kind}"`)
    return fallbackName
  }

  return t(key)
}
