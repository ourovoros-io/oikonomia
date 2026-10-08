import { readFile } from 'node:fs/promises'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { ReportLine } from './api'
import { PDFDocument, PDFArray, PDFDict, PDFName, PDFPage, PDFRawStream, decodePDFRawStream } from 'pdf-lib'
import {
  A4_HEIGHT,
  A4_WIDTH,
  BRAND_MARK,
  annularPath,
  buildExpensePdfBytes,
  buildExpensePdfModel,
  buildExpenseReportSvg,
  bytesToBase64,
  formatPdfPeriod,
  pdfExportErrorMessage,
  roundedRectPath,
  suggestedExpensePdfName,
} from './expensePdf'
import { LOCALES, resetI18nForTests, setLocale, t, type Locale } from './i18n'

// The production code fetches the bundled fonts by URL, which does not resolve
// in the node test environment. Serve the real .ttf files from disk instead so
// the tests exercise the same embedding path as the app.
const FONT_FILES: Record<string, string> = {
  'Inter-Regular.ttf': 'Inter-Regular.ttf',
  'Inter-SemiBold.ttf': 'Inter-SemiBold.ttf',
}

beforeEach(() => {
  vi.stubGlobal('fetch', async (url: string) => {
    const name = Object.keys(FONT_FILES).find((file) => String(url).includes(file))
    if (!name) return new Response(null, { status: 404 })
    const bytes = await readFile(new URL(`../assets/fonts/${FONT_FILES[name]}`, import.meta.url))
    return new Response(new Uint8Array(bytes))
  })
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
  resetI18nForTests()
})

/** Base font names of every font object in a built PDF. */
async function embeddedFontNames(bytes: Uint8Array): Promise<string[]> {
  const doc = await PDFDocument.load(bytes)
  const names: string[] = []
  for (const [, object] of doc.context.enumerateIndirectObjects()) {
    if (!(object instanceof PDFDict)) continue
    if (object.get(PDFName.of('Type')) !== PDFName.of('Font')) continue
    const base = object.get(PDFName.of('BaseFont'))
    if (base instanceof PDFName) names.push(base.decodeText())
  }
  return names
}

/** The page content of the first page, inflated. */
async function decodedContent(doc: PDFDocument): Promise<string> {
  const contents = doc.getPage(0).node.Contents()
  const streams = contents instanceof PDFArray ? contents.asArray() : [contents]
  const parts = streams.map((ref) => {
    const stream = doc.context.lookup(ref)
    return stream instanceof PDFRawStream ? decodePDFRawStream(stream).decode() : new Uint8Array()
  })
  return parts.map((bytes) => new TextDecoder().decode(bytes)).join('\n')
}

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
  currency: { code: 'EUR', decimals: 2 },
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

/** The numbers of an SVG path made of absolute commands, as (x, y) pairs. */
function pathPoints(path: string): Array<{ x: number; y: number }> {
  const numbers = path.match(/-?\d+(?:\.\d+)?(?:e-?\d+)?/g)?.map(Number) ?? []
  const points: Array<{ x: number; y: number }> = []
  for (let i = 0; i + 1 < numbers.length; i += 2) points.push({ x: numbers[i], y: numbers[i + 1] })
  return points
}

describe('shapes in page-top coordinates', () => {
  test('a ring segment stays inside its circle and starts at the top', () => {
    const path = annularPath(200, 380, 82, 58, -Math.PI / 2, 0)
    const [start] = pathPoints(path)

    expect(start.x).toBeCloseTo(200, 3)
    expect(start.y).toBeCloseTo(380 - 82, 3)
    // Arc commands carry radii and flags, which are not points; the end
    // points of the two lines are.
    expect(path).toContain('A 82 82 0 0 1 282 380')
    expect(path).toMatch(/A 58 58 0 0 0 200(\.\d+)? 322/)
  })

  test('a ring of one slice is cut short of a whole turn so it still draws', () => {
    const path = annularPath(200, 380, 82, 58, -Math.PI / 2, (3 * Math.PI) / 2)
    const [outerStart] = pathPoints(path)
    const outerEnd = path.match(/A 82 82 0 1 1 (\S+) (\S+)/)

    expect(outerEnd).not.toBeNull()
    expect(Number(outerEnd?.[2])).not.toBeCloseTo(outerStart.y, 6)
  })

  test('a rounded rectangle spans exactly its box', () => {
    expect(roundedRectPath(44, 248, 507, 268, 14)).toBe(
      'M 58 248 H 537 Q 551 248 551 262 V 502 Q 551 516 537 516 H 58 Q 44 516 44 502 V 262 Q 44 248 58 248 Z',
    )
  })
})

describe('the report is drawn on the page', () => {
  test('every shape is anchored at the top-left corner, not below the page', async () => {
    const drawn: Array<{ path: string; y: number | undefined }> = []
    const original = PDFPage.prototype.drawSvgPath
    vi.spyOn(PDFPage.prototype, 'drawSvgPath').mockImplementation(function (this: PDFPage, path, options) {
      drawn.push({ path, y: options?.y })
      return original.call(this, path, options)
    })

    await buildExpensePdfBytes(filled)

    const shapes = drawn.filter((call) => !Object.values(BRAND_MARK).includes(call.path as never))
    // Cards, legend dots and the ring segments.
    expect(shapes.length).toBeGreaterThan(filled.expenses.length)
    for (const shape of shapes) expect(shape.y, shape.path).toBe(A4_HEIGHT)
  })

  test('the brand mark sits inside the logo tile, not a tile below it', async () => {
    const marks: number[] = []
    const original = PDFPage.prototype.drawSvgPath
    vi.spyOn(PDFPage.prototype, 'drawSvgPath').mockImplementation(function (this: PDFPage, path, options) {
      if (path === BRAND_MARK.shield) marks.push(options?.y ?? Number.NaN)
      return original.call(this, path, options)
    })

    await buildExpensePdfBytes(filled)

    // The header mark: its top edge is the tile's top edge (44 + 2 from the page top).
    expect(marks[0]).toBeCloseTo(A4_HEIGHT - 46, 3)
  })

  test('the page is white, for printing', async () => {
    const bytes = await buildExpensePdfBytes(filled)
    const doc = await PDFDocument.load(bytes)
    const content = await decodedContent(doc)

    // The first fill is the page: white, full page.
    expect(content).toMatch(/1 1 1 rg/)
    expect(content).not.toMatch(/0\.0392 0\.0549 0\.0431 rg/)
  })
})

describe('formatPdfPeriod', () => {
  test('collapses a same-month range like the mock', () => {
    expect(formatPdfPeriod('2026-08-01', '2026-08-31', 'en')).toBe('1 – 31 Aug 2026')
  })
})

describe('buildExpenseReportSvg', () => {
  test('paints the light A4 filled report tokens', () => {
    const svg = buildExpenseReportSvg(filled)
    expect(svg).toContain('viewBox="0 0 595.28 841.89"')
    expect(svg).toContain('fill="#ffffff"')
    expect(svg).toContain('#eef2ef')
    expect(svg).toContain('#f6f8f6')
    expect(svg).toContain('#1f8a4c')
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
    expect(svg).toContain(BRAND_MARK.shield)
    expect(svg).toContain(BRAND_MARK.pediment)
    expect(svg).toContain(BRAND_MARK.house)
    expect(svg).toContain('rx="2"')
    expect(svg).toContain(t('reports.pdf.sliceNote'))
  })

  test('empty period still builds the dashed empty state', () => {
    const svg = buildExpenseReportSvg({
      entityName: 'Personal',
      currency: { code: 'EUR', decimals: 2 },
      from: '2026-08-01',
      to: '2026-08-31',
      expenses: [],
    })
    expect(svg).toContain('No expenses in this period')
    expect(svg).toContain('Nothing to chart for 1 – 31 Aug 2026')
    expect(svg).toContain('stroke-dasharray')
    const model = buildExpensePdfModel({
      entityName: 'Personal',
      currency: { code: 'EUR', decimals: 2 },
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
      currency: { code: 'EUR', decimals: 2 },
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

describe('buildExpensePdfBytes in every language', () => {
  const names: Record<Locale, { entity: string; categories: [string, string] }> = {
    en: { entity: 'Personal', categories: ['Rent', 'Groceries'] },
    el: { entity: 'Προσωπικά', categories: ['Ενοίκιο', 'Τρόφιμα'] },
    fr: { entity: 'Personnel', categories: ['Loyer', 'Épicerie'] },
    de: { entity: 'Privat', categories: ['Miete', 'Lebensmittel'] },
  }

  test.each(LOCALES)('%s report builds and embeds Inter', async (locale) => {
    setLocale(locale)
    const { entity, categories } = names[locale]
    const bytes = await buildExpensePdfBytes({
      ...filled,
      entityName: entity,
      expenses: [
        expense({ code: '6100', name: categories[0], balance_minor: 850_00 }),
        expense({ code: '6200', name: categories[1], balance_minor: 420_00 }),
      ],
    })

    expect(new TextDecoder().decode(bytes.slice(0, 5))).toBe('%PDF-')
    expect(bytes.length).toBeGreaterThan(500)
    const fonts = await embeddedFontNames(bytes)
    expect(fonts.some((name) => name.includes('Inter-Regular'))).toBe(true)
    expect(fonts.some((name) => name.includes('Inter-SemiBold'))).toBe(true)
    expect(fonts.some((name) => name.includes('Helvetica'))).toBe(false)
  })

  test('Greek entity and category names build while the UI is English', async () => {
    setLocale('en')
    const bytes = await buildExpensePdfBytes({
      ...filled,
      entityName: 'Οικονομικά',
      expenses: [
        expense({ code: '6200', name: 'Τρόφιμα', balance_minor: 420_00 }),
        expense({ code: '6100', name: 'Rent', balance_minor: 850_00 }),
      ],
    })

    expect(new TextDecoder().decode(bytes.slice(0, 5))).toBe('%PDF-')
  })

  test('empty Greek report builds', async () => {
    setLocale('el')
    const bytes = await buildExpensePdfBytes({ ...filled, expenses: [] })

    expect(new TextDecoder().decode(bytes.slice(0, 5))).toBe('%PDF-')
  })

  test('warns and keeps Helvetica when the font files cannot be loaded', async () => {
    vi.stubGlobal('fetch', async () => new Response(new Uint8Array([1, 2, 3])))
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})

    const bytes = await buildExpensePdfBytes(filled)

    expect(warn).toHaveBeenCalledTimes(1)
    expect((await embeddedFontNames(bytes)).some((name) => name.includes('Helvetica'))).toBe(true)
  })
})

describe('pdfExportErrorMessage', () => {
  test('logs the raw cause of an export failure, which the sentence hides', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})

    pdfExportErrorMessage({ code: 'save_failed', message: 'ENOSPC /tmp/x.pdf' })
    pdfExportErrorMessage({ code: 'io', message: 'EACCES /tmp/y.pdf' })

    expect(warn).toHaveBeenCalledTimes(2)
    expect(warn.mock.calls[0][0]).toContain('ENOSPC /tmp/x.pdf')
    expect(warn.mock.calls[1][0]).toContain('EACCES /tmp/y.pdf')
  })

  test('maps io by code only', () => {
    expect(pdfExportErrorMessage({ code: 'io', message: 'EACCES /tmp/x.pdf' })).toBe(
      'Could not export the PDF.',
    )
    for (const code of ['save_location_invalid', 'save_failed', 'file_data_invalid', 'file_too_large']) {
      expect(pdfExportErrorMessage({ code, message: 'invalid file data' })).toBe(
        'Could not export the PDF.',
      )
    }
    expect(pdfExportErrorMessage({ code: 'vault_locked', message: 'Vault is locked' })).toBe(
      'The vault is locked.',
    )
  })

  test('an unknown code shows the PDF fallback, never the raw message', () => {
    expect(pdfExportErrorMessage({ code: 'brand_new', message: 'EACCES /tmp/x.pdf' })).toBe(
      'Could not export the PDF.',
    )
  })
})

describe('EL PDF copy', () => {
  test('uses formal σας and keeps the Oikonomia brand', () => {
    setLocale('el')
    const model = buildExpensePdfModel({
      entityName: 'Προσωπικό',
      currency: { code: 'EUR', decimals: 2 },
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
