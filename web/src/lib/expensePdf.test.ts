import { afterEach, describe, expect, test } from 'vitest'
import type { ReportLine } from './api'
import { PDFDocument } from 'pdf-lib'
import {
  A4_HEIGHT,
  A4_WIDTH,
  buildExpensePdfBytes,
  buildExpensePdfModel,
  buildExpenseReportSvg,
  bytesToBase64,
  formatPdfPeriod,
  pdfExportErrorMessage,
  suggestedExpensePdfName,
} from './expensePdf'
import { resetI18nForTests, setLocale } from './i18n'

afterEach(() => {
  resetI18nForTests()
})

function expense(
  over: Partial<ReportLine> & Pick<ReportLine, 'code' | 'name' | 'balance_minor'>,
): ReportLine {
  return {
    account_type: 'expense',
    debit_minor: over.balance_minor,
    credit_minor: 0,
    ...over,
  }
}

const filled = {
  entityName: 'Personal',
  currency: 'EUR',
  from: '2026-08-01',
  to: '2026-08-31',
  expenses: [
    expense({ code: '6100', name: 'Rent', balance_minor: 850_00 }),
    expense({ code: '6200', name: 'Groceries', balance_minor: 420_00 }),
    expense({ code: '6300', name: 'Dining', balance_minor: 285_00 }),
    expense({ code: '6400', name: 'Transport', balance_minor: 210_00 }),
    expense({ code: '6500', name: 'Utilities', balance_minor: 165_00 }),
    expense({ code: '6600', name: 'Subscriptions', balance_minor: 92_00 }),
    expense({ code: '6700', name: 'Health', balance_minor: 42_00 }),
    expense({ code: '6800', name: 'Clothing', balance_minor: 24_00 }),
    expense({ code: '6900', name: 'Misc', balance_minor: 12_00 }),
  ],
}

describe('suggestedExpensePdfName', () => {
  test('sanitizes the period into oikonomia-expenses-{from}_{to}.pdf', () => {
    expect(suggestedExpensePdfName('2026-08-01', '2026-08-31')).toBe(
      'oikonomia-expenses-2026-08-01_2026-08-31.pdf',
    )
    expect(suggestedExpensePdfName('2026/08/01 12:00', 'x y')).toBe(
      'oikonomia-expenses-2026-08-01-12-00_x-y.pdf',
    )
  })
})

describe('formatPdfPeriod', () => {
  test('collapses a same-month range like the mock', () => {
    expect(formatPdfPeriod('2026-08-01', '2026-08-31', 'en')).toBe('1 – 31 Aug 2026')
  })
})

describe('buildExpenseReportSvg', () => {
  test('paints the dark A4 filled mock tokens', () => {
    const svg = buildExpenseReportSvg(filled)
    expect(svg).toContain('viewBox="0 0 595.28 841.89"')
    expect(svg).toContain('#0a0e0b')
    expect(svg).toContain('#101511')
    expect(svg).toContain('#151b16')
    expect(svg).toContain('#35b06b')
    expect(svg).toContain('#3987e5')
    expect(svg).toContain('#4a554c')
    expect(svg).toContain('Monthly expenses')
    expect(svg).toContain('1 – 31 Aug 2026')
    expect(svg).toContain('Personal · EUR')
    expect(svg).toContain('Oikonomia')
    expect(svg).toContain('Your books never leave this computer')
    expect(svg).toContain('Oikonomia · local report')
    expect(svg).toContain('Other (3 categories)')
    expect(svg).toContain('Hidden entries omitted')
  })

  test('empty period still builds the dashed empty state', () => {
    const svg = buildExpenseReportSvg({
      entityName: 'Personal',
      currency: 'EUR',
      from: '2026-08-01',
      to: '2026-08-31',
      expenses: [],
    })
    expect(svg).toContain('No expenses in this period')
    expect(svg).toContain('Nothing to chart for 1 – 31 Aug 2026')
    expect(svg).toContain('stroke-dasharray')
    const model = buildExpensePdfModel({
      entityName: 'Personal',
      currency: 'EUR',
      from: '2026-08-01',
      to: '2026-08-31',
      expenses: [],
    })
    expect(model.slices).toEqual([])
    expect(model.total).toBe(0)
    expect(model.labels.totalAmount).toMatch(/0/)
  })
})

describe('buildExpensePdfBytes', () => {
  test('empty period still produces a PDF', async () => {
    const bytes = await buildExpensePdfBytes({
      entityName: 'Personal',
      currency: 'EUR',
      from: '2026-08-01',
      to: '2026-08-31',
      expenses: [],
    })
    const head = new TextDecoder().decode(bytes.slice(0, 8))
    expect(head.startsWith('%PDF-')).toBe(true)
    expect(bytes.length).toBeGreaterThan(500)
    const doc = await PDFDocument.load(bytes)
    const page = doc.getPage(0)
    expect(page.getWidth()).toBeCloseTo(A4_WIDTH, 1)
    expect(page.getHeight()).toBeCloseTo(A4_HEIGHT, 1)
  })

  test('filled period produces PDF bytes that encode as base64', async () => {
    const bytes = await buildExpensePdfBytes(filled)
    expect(new TextDecoder().decode(bytes.slice(0, 5))).toBe('%PDF-')
    const b64 = bytesToBase64(bytes)
    expect(b64.length).toBeGreaterThan(80)
    expect(Buffer.from(b64, 'base64').subarray(0, 5).toString()).toBe('%PDF-')
  })
})

describe('pdfExportErrorMessage', () => {
  test('maps io by code only', () => {
    expect(pdfExportErrorMessage({ code: 'io', message: 'EACCES /tmp/x.pdf' })).toBe(
      'Could not export PDF',
    )
    expect(pdfExportErrorMessage({ code: 'validation', message: 'invalid file data' })).toBe(
      'Could not export PDF',
    )
    expect(pdfExportErrorMessage({ code: 'vault_locked', message: 'Vault is locked' })).toBe(
      'Vault is locked',
    )
  })
})

describe('EL PDF copy', () => {
  test('uses formal σας and keeps the Oikonomia brand', () => {
    setLocale('el')
    const model = buildExpensePdfModel({
      entityName: 'Προσωπικό',
      currency: 'EUR',
      from: '2026-08-01',
      to: '2026-08-31',
      expenses: [],
    })
    expect(model.labels.title).toBe('Μηνιαία έξοδα')
    expect(model.labels.footerPrivacy).toContain('σας')
    expect(model.labels.footerLocal).toContain('Oikonomia')
    expect(model.labels.emptyBody).toContain(model.period)
  })
})
