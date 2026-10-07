import fontkit from '@pdf-lib/fontkit'
import { LineCapStyle, PDFDocument, StandardFonts, rgb } from 'pdf-lib'
import type { PDFFont, PDFPage } from 'pdf-lib'
import interRegularUrl from '../assets/fonts/Inter-Regular.ttf?url'
import interSemiBoldUrl from '../assets/fonts/Inter-SemiBold.ttf?url'
import type { ReportLine } from './api'
import { asCommandError, commandErrorMessage, logCommandError } from './commandError'
import { formatMoney, type Currency } from './money'
import { buildSlices, vizHex, type ExpenseSlice } from './expenseSlices'
import { getLocale, t, type Locale } from './i18n'

/** A4 in PDF points. */
export const A4_WIDTH = 595.28
export const A4_HEIGHT = 841.89

const CANVAS = '#0a0e0b'
const SURFACE = '#101511'
const SURFACE_2 = '#151b16'
const ACCENT = '#35b06b'
const FG = '#f4f6f4'
const MUTED = '#8f9a93'
const EMPTY_RING = '#2a332e'

const MARGIN = 44
const LOGO = 28

const EN_MON = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
const EL_MON = ['Ιαν', 'Φεβ', 'Μαρ', 'Απρ', 'Μάι', 'Ιουν', 'Ιουλ', 'Αυγ', 'Σεπ', 'Οκτ', 'Νοε', 'Δεκ']

export type ExpensePdfInput = {
  entityName: string
  currency: Currency
  from: string
  to: string
  expenses: ReportLine[]
}

export type ExpensePdfModel = {
  entityName: string
  currency: Currency
  period: string
  slices: ExpenseSlice[]
  total: number
  labels: {
    brand: string
    context: string
    title: string
    whisper: string
    totalExpenses: string
    totalAmount: string
    emptyTitle: string
    emptyBody: string
    sliceNote: string
    footerPrivacy: string
    footerLocal: string
  }
}

function hexRgb(hex: string) {
  const n = hex.replace('#', '')
  return rgb(
    Number.parseInt(n.slice(0, 2), 16) / 255,
    Number.parseInt(n.slice(2, 4), 16) / 255,
    Number.parseInt(n.slice(4, 6), 16) / 255,
  )
}

function isoParts(iso: string): { y: number; m: number; d: number } | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso)
  if (!m) return null
  return { y: Number(m[1]), m: Number(m[2]), d: Number(m[3]) }
}

function monthShort(month: number, locale: Locale): string {
  const names = locale === 'el' ? EL_MON : EN_MON
  return names[month - 1] ?? String(month)
}

/** Report period like `1 – 31 Aug 2026`. */
export function formatPdfPeriod(from: string, to: string, locale: Locale = getLocale()): string {
  const a = isoParts(from)
  const b = isoParts(to)
  if (!a || !b) return `${from} – ${to}`
  const aMon = monthShort(a.m, locale)
  const bMon = monthShort(b.m, locale)
  if (a.y === b.y && a.m === b.m) return `${a.d} – ${b.d} ${aMon} ${a.y}`
  if (a.y === b.y) return `${a.d} ${aMon} – ${b.d} ${bMon} ${a.y}`
  return `${a.d} ${aMon} ${a.y} – ${b.d} ${bMon} ${b.y}`
}

/** Default save name: `oikonomia-expenses-{from}_{to}.pdf`. */
export function suggestedExpensePdfName(from: string, to: string): string {
  const clean = (value: string) =>
    value.replace(/[^0-9A-Za-z._-]+/g, '-').replace(/^-+|-+$/g, '') || 'date'
  return `oikonomia-expenses-${clean(from)}_${clean(to)}.pdf`
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

export function buildExpensePdfModel(input: ExpensePdfInput): ExpensePdfModel {
  const locale = getLocale()
  const period = formatPdfPeriod(input.from, input.to, locale)
  const moneyLocale = locale === 'el' ? 'el-GR' : 'en-US'
  const { slices, total } = buildSlices(input.expenses)
  return {
    entityName: input.entityName,
    currency: input.currency,
    period,
    slices,
    total,
    labels: {
      brand: 'Oikonomia',
      context: `${input.entityName} · ${input.currency.code}`,
      title: t('reports.pdf.title'),
      whisper: t('reports.pdf.meta', { currency: input.currency.code }),
      totalExpenses: t('reports.pdf.totalExpenses'),
      totalAmount: formatMoney(total, input.currency, moneyLocale),
      emptyTitle: t('reports.pdf.emptyTitle'),
      emptyBody: t('reports.pdf.emptyBody', { period }),
      sliceNote: t('reports.pdf.sliceNote'),
      footerPrivacy: t('reports.pdf.footerPrivacy'),
      footerLocal: t('reports.pdf.footerLocal'),
    },
  }
}

function escapeXml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function svgDonut(cx: number, cy: number, slices: ExpenseSlice[]): string {
  const r = 70
  const stroke = 22
  const circ = 2 * Math.PI * r
  const gap = slices.length > 1 ? 2 : 0
  let acc = 0
  const rings = slices.map((slice) => {
    const full = slice.share * circ
    const start = acc
    acc += full
    const length = Math.max(full - gap, 0.5)
    return `<circle cx="${cx}" cy="${cy}" r="${r}" fill="none" stroke="${vizHex(slice.slot)}" stroke-width="${stroke}" stroke-dasharray="${length} ${circ - length}" stroke-dashoffset="${-start}" transform="rotate(-90 ${cx} ${cy})"/>`
  })
  return rings.join('')
}

function svgEmptyRing(cx: number, cy: number): string {
  return `<circle cx="${cx}" cy="${cy}" r="70" fill="none" stroke="${EMPTY_RING}" stroke-width="22"/>`
}

/** House-in-shield mark in 24×24 lucide space (same art as `svgShield` / Logo). */
export const BRAND_MARK = {
  shield:
    'M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1 1 0 0 1 1.52 0C14.5 3.8 17 5 19 5a1 1 0 0 1 1 1z',
  pediment: 'M8.2 11.2 L12 8.4 L15.8 11.2',
  house: 'M9.2 11.4 h5.6 v5.2 h-2.05 v-2.1 a1.15 1.15 0 1 0-1.5 0 v2.1 H9.2 z',
} as const

function svgShield(x: number, y: number, size: number): string {
  const s = size / 24
  return `<g transform="translate(${x} ${y}) scale(${s})">
    <path d="${BRAND_MARK.shield}" fill="none" stroke="${ACCENT}" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="${BRAND_MARK.pediment}" fill="none" stroke="${ACCENT}" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="${BRAND_MARK.house}" fill="${ACCENT}"/>
  </g>`
}

/** Flip a 24-unit y-down SVG path into PDF y-up so drawSvgPath matches `svgShield`. */
export function flipSvgPathY(d: string, box = 24): string {
  const tokens = d.match(/[A-Za-z]|[-+]?(?:\d*\.\d+|\d+)(?:[eE][-+]?\d+)?/g)
  if (!tokens) return d
  let i = 0
  let cmd = ''
  const out: string[] = []
  const num = () => Number(tokens[i++])
  const push = (...xs: Array<string | number>) => {
    for (const x of xs) out.push(String(x))
  }
  while (i < tokens.length) {
    const tok = tokens[i]
    if (/^[A-Za-z]$/.test(tok)) {
      cmd = tok
      out.push(tok)
      i += 1
      continue
    }
    switch (cmd) {
      case 'H':
      case 'h':
        push(num())
        break
      case 'V':
        push(box - num())
        break
      case 'v':
        push(-num())
        break
      case 'A': {
        const rx = num()
        const ry = num()
        const rot = num()
        const large = num()
        const sweep = num()
        const x = num()
        const y = num()
        push(rx, ry, rot, large, sweep ? 0 : 1, x, box - y)
        break
      }
      case 'a': {
        const rx = num()
        const ry = num()
        const rot = num()
        const large = num()
        const sweep = num()
        const dx = num()
        const dy = num()
        push(rx, ry, rot, large, sweep ? 0 : 1, dx, -dy)
        break
      }
      case 'C':
      case 'c':
      case 'S':
      case 's':
      case 'Q':
      case 'q':
      case 'T':
      case 't':
      case 'M':
      case 'm':
      case 'L':
      case 'l': {
        const pair = cmd === 'C' || cmd === 'c' ? 3 : cmd === 'S' || cmd === 's' || cmd === 'Q' || cmd === 'q' ? 2 : 1
        const rel = cmd === cmd.toLowerCase()
        for (let p = 0; p < pair; p += 1) {
          const x = num()
          const y = num()
          push(x, rel ? -y : box - y)
        }
        break
      }
      default:
        push(num())
    }
  }
  return out.join(' ')
}

function drawBrandMark(page: PDFPage, x: number, yTop: number, size: number) {
  const s = size / 24
  const originY = yPdf(yTop + size)
  const stroke = {
    borderColor: hexRgb(ACCENT),
    borderLineCap: LineCapStyle.Round,
    x,
    y: originY,
    scale: s,
  }
  page.drawSvgPath(flipSvgPathY(BRAND_MARK.shield), { ...stroke, borderWidth: 1.4 })
  page.drawSvgPath(flipSvgPathY(BRAND_MARK.pediment), { ...stroke, borderWidth: 1.3 })
  page.drawSvgPath(flipSvgPathY(BRAND_MARK.house), {
    x,
    y: originY,
    scale: s,
    color: hexRgb(ACCENT),
  })
}

/** Dark A4 SVG matching the owner-greenlit monthly-expenses mocks. */
export function buildExpenseReportSvg(input: ExpensePdfInput): string {
  const model = buildExpensePdfModel(input)
  const { labels, slices, period } = model
  const empty = slices.length === 0
  const cardTop = 248
  const cardH = empty ? 320 : 268
  const cardW = A4_WIDTH - MARGIN * 2
  const cx = MARGIN + 150
  const cy = cardTop + cardH / 2
  const legendX = MARGIN + 290
  const legendY = cardTop + 36

  const legend = slices
    .map((slice, i) => {
      const y = legendY + i * 28
      const pct = `${(slice.share * 100).toFixed(1)}%`
      return `<g>
        <rect x="${legendX - 4}" y="${y - 4}" width="8" height="8" rx="2" fill="${vizHex(slice.slot)}"/>
        <text x="${legendX + 14}" y="${y + 4}" fill="${FG}" font-size="12">${escapeXml(slice.name)}</text>
        <text x="${MARGIN + cardW - 72}" y="${y + 4}" fill="${FG}" font-size="12" font-weight="600" text-anchor="end">${escapeXml(formatMoney(slice.amount, model.currency, getLocale() === 'el' ? 'el-GR' : 'en-US'))}</text>
        <text x="${MARGIN + cardW - 16}" y="${y + 4}" fill="${MUTED}" font-size="11" text-anchor="end">${pct}</text>
      </g>`
    })
    .join('')

  const body = empty
    ? `<rect x="${MARGIN}" y="${cardTop}" width="${cardW}" height="${cardH}" rx="14" fill="none" stroke="#3a433c" stroke-dasharray="5 5"/>
       ${svgEmptyRing(A4_WIDTH / 2, cardTop + 118)}
       <text x="${A4_WIDTH / 2}" y="${cardTop + 210}" fill="${FG}" font-size="16" font-weight="700" text-anchor="middle">${escapeXml(labels.emptyTitle)}</text>
       <text x="${A4_WIDTH / 2}" y="${cardTop + 234}" fill="${MUTED}" font-size="11" text-anchor="middle">${escapeXml(labels.emptyBody)}</text>`
    : `<rect x="${MARGIN}" y="${cardTop}" width="${cardW}" height="${cardH}" rx="14" fill="${SURFACE_2}"/>
       ${svgDonut(cx, cy, slices)}
       <text x="${cx}" y="${cy - 8}" fill="${MUTED}" font-size="8" font-weight="600" letter-spacing="0.08em" text-anchor="middle">${escapeXml(labels.totalExpenses.toUpperCase())}</text>
       <text x="${cx}" y="${cy + 14}" fill="${FG}" font-size="16" font-weight="700" text-anchor="middle">${escapeXml(labels.totalAmount)}</text>
       ${legend}`

  const barY = cardTop + cardH + 16
  const note = empty
    ? ''
    : `<text x="${MARGIN}" y="${barY + 68}" fill="${MUTED}" font-size="9">${escapeXml(labels.sliceNote)}</text>`

  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${A4_WIDTH} ${A4_HEIGHT}" width="${A4_WIDTH}" height="${A4_HEIGHT}">
  <rect width="${A4_WIDTH}" height="${A4_HEIGHT}" fill="${CANVAS}"/>
  <rect x="${MARGIN}" y="${MARGIN}" width="${LOGO}" height="${LOGO}" rx="7" fill="${SURFACE}"/>
  ${svgShield(MARGIN + 2, MARGIN + 2, 24)}
  <text x="${MARGIN + LOGO + 10}" y="${MARGIN + 13}" fill="${FG}" font-size="15" font-weight="700">Oikonomia</text>
  <text x="${MARGIN + LOGO + 10}" y="${MARGIN + 27}" fill="${MUTED}" font-size="10">${escapeXml(labels.context)}</text>
  <rect x="${MARGIN}" y="${MARGIN + 40}" width="168" height="1.5" fill="${ACCENT}"/>
  <text x="${MARGIN}" y="132" fill="${FG}" font-size="28" font-weight="700">${escapeXml(labels.title)}</text>
  <text x="${MARGIN}" y="158" fill="${FG}" font-size="14">${escapeXml(period)}</text>
  <text x="${MARGIN}" y="180" fill="${MUTED}" font-size="10">${escapeXml(labels.whisper)}</text>
  ${body}
  <rect x="${MARGIN}" y="${barY}" width="${cardW}" height="44" rx="10" fill="${SURFACE}"/>
  <text x="${MARGIN + 16}" y="${barY + 27}" fill="${empty ? MUTED : FG}" font-size="12">${escapeXml(labels.totalExpenses)}</text>
  <text x="${MARGIN + cardW - 16}" y="${barY + 28}" fill="${FG}" font-size="16" font-weight="700" text-anchor="end">${escapeXml(labels.totalAmount)}</text>
  ${note}
  <rect x="${MARGIN}" y="${A4_HEIGHT - 52}" width="${cardW}" height="1" fill="#232c26"/>
  ${svgShield(MARGIN, A4_HEIGHT - 38, 12)}
  <text x="${MARGIN + 18}" y="${A4_HEIGHT - 26}" fill="${MUTED}" font-size="9">${escapeXml(labels.footerPrivacy)}</text>
  <text x="${A4_WIDTH - MARGIN}" y="${A4_HEIGHT - 26}" fill="${MUTED}" font-size="9" text-anchor="end">${escapeXml(labels.footerLocal)}</text>
</svg>`
}

function yPdf(yTop: number): number {
  return A4_HEIGHT - yTop
}

function roundedRect(
  page: PDFPage,
  x: number,
  yTop: number,
  w: number,
  h: number,
  r: number,
  fill: string,
  stroke?: { color: string; width: number; dash?: number[] },
) {
  const y = yPdf(yTop + h)
  const path = [
    `M ${x + r} ${y}`,
    `H ${x + w - r}`,
    `Q ${x + w} ${y} ${x + w} ${y + r}`,
    `V ${y + h - r}`,
    `Q ${x + w} ${y + h} ${x + w - r} ${y + h}`,
    `H ${x + r}`,
    `Q ${x} ${y + h} ${x} ${y + h - r}`,
    `V ${y + r}`,
    `Q ${x} ${y} ${x + r} ${y}`,
    'Z',
  ].join(' ')
  page.drawSvgPath(path, stroke
    ? {
        borderColor: hexRgb(stroke.color),
        borderWidth: stroke.width,
        borderDashArray: stroke.dash,
      }
    : { color: hexRgb(fill) })
}

function wrapLines(font: PDFFont, text: string, size: number, maxWidth: number): string[] {
  const words = text.split(/\s+/).filter(Boolean)
  const lines: string[] = []
  let current = ''
  for (const word of words) {
    const next = current ? `${current} ${word}` : word
    if (current && font.widthOfTextAtSize(next, size) > maxWidth) {
      lines.push(current)
      current = word
    } else {
      current = next
    }
  }
  if (current) lines.push(current)
  return lines
}

function drawText(
  page: PDFPage,
  font: PDFFont,
  text: string,
  x: number,
  yTop: number,
  size: number,
  color: string,
  opts?: { align?: 'left' | 'right' | 'center'; maxWidth?: number },
) {
  let draw = text
  if (opts?.maxWidth) {
    while (draw.length > 1 && font.widthOfTextAtSize(draw, size) > opts.maxWidth) {
      draw = `${draw.slice(0, -2)}…`
    }
  }
  let dx = x
  const w = font.widthOfTextAtSize(draw, size)
  if (opts?.align === 'right') dx = x - w
  if (opts?.align === 'center') dx = x - w / 2
  page.drawText(draw, {
    x: dx,
    y: yPdf(yTop) - size * 0.2,
    size,
    font,
    color: hexRgb(color),
  })
}

function annularPath(
  cx: number,
  cyTop: number,
  rOut: number,
  rIn: number,
  a0: number,
  a1: number,
): string {
  const cy = yPdf(cyTop)
  const p = (r: number, a: number) => ({
    x: cx + r * Math.cos(a),
    y: cy + r * Math.sin(-a),
  })
  const large = Math.abs(a1 - a0) > Math.PI ? 1 : 0
  const o0 = p(rOut, a0)
  const o1 = p(rOut, a1)
  const i1 = p(rIn, a1)
  const i0 = p(rIn, a0)
  return [
    `M ${o0.x} ${o0.y}`,
    `A ${rOut} ${rOut} 0 ${large} 1 ${o1.x} ${o1.y}`,
    `L ${i1.x} ${i1.y}`,
    `A ${rIn} ${rIn} 0 ${large} 0 ${i0.x} ${i0.y}`,
    'Z',
  ].join(' ')
}

async function embedReportFonts(pdf: PDFDocument): Promise<{ regular: PDFFont; semibold: PDFFont }> {
  try {
    const [regularBytes, semiboldBytes] = await Promise.all([
      loadFontBytes(interRegularUrl),
      loadFontBytes(interSemiBoldUrl),
    ])
    if (!regularBytes || !semiboldBytes) {
      throw new Error('the bundled Inter font files could not be loaded')
    }

    // pdf-lib can only embed a custom TrueType font once fontkit is registered.
    pdf.registerFontkit(fontkit)

    return {
      regular: await pdf.embedFont(regularBytes, { subset: true }),
      semibold: await pdf.embedFont(semiboldBytes, { subset: true }),
    }
  } catch (cause) {
    // Helvetica covers plain Latin-1 text only; Greek, other scripts and U+202F will fail.
    console.warn('Could not embed the Inter font in the expense PDF; using Helvetica.', cause)
  }

  return {
    regular: await pdf.embedFont(StandardFonts.Helvetica),
    semibold: await pdf.embedFont(StandardFonts.HelveticaBold),
  }
}

async function loadFontBytes(url: string): Promise<Uint8Array | null> {
  try {
    const res = await fetch(url)
    if (!res.ok) return null
    return new Uint8Array(await res.arrayBuffer())
  } catch {
    return null
  }
}

function paintPdfPage(
  page: PDFPage,
  model: ExpensePdfModel,
  fonts: { regular: PDFFont; semibold: PDFFont },
) {
  const { labels, slices, period } = model
  const empty = slices.length === 0
  page.drawRectangle({
    x: 0,
    y: 0,
    width: A4_WIDTH,
    height: A4_HEIGHT,
    color: hexRgb(CANVAS),
  })

  roundedRect(page, MARGIN, MARGIN, LOGO, LOGO, 7, SURFACE)
  drawBrandMark(page, MARGIN + 2, MARGIN + 2, 24)

  drawText(page, fonts.semibold, labels.brand, MARGIN + LOGO + 10, MARGIN + 13, 15, FG)
  drawText(page, fonts.regular, labels.context, MARGIN + LOGO + 10, MARGIN + 27, 10, MUTED)
  page.drawRectangle({
    x: MARGIN,
    y: yPdf(MARGIN + 42),
    width: 168,
    height: 1.6,
    color: hexRgb(ACCENT),
  })

  drawText(page, fonts.semibold, labels.title, MARGIN, 132, 28, FG)
  drawText(page, fonts.regular, period, MARGIN, 158, 14, FG)
  drawText(page, fonts.regular, labels.whisper, MARGIN, 180, 10, MUTED, {
    maxWidth: A4_WIDTH - MARGIN * 2,
  })

  const cardTop = 248
  const cardH = empty ? 320 : 268
  const cardW = A4_WIDTH - MARGIN * 2
  const cx = MARGIN + 150
  const cy = cardTop + cardH / 2

  if (empty) {
    roundedRect(page, MARGIN, cardTop, cardW, cardH, 14, CANVAS, {
      color: '#3a433c',
      width: 1,
      dash: [5, 5],
    })
    page.drawCircle({
      x: A4_WIDTH / 2,
      y: yPdf(cardTop + 118),
      size: 70,
      borderColor: hexRgb(EMPTY_RING),
      borderWidth: 22,
    })
    drawText(page, fonts.semibold, labels.emptyTitle, A4_WIDTH / 2, cardTop + 210, 16, FG, {
      align: 'center',
      maxWidth: cardW - 48,
    })
    wrapLines(fonts.regular, labels.emptyBody, 11, cardW - 64).forEach((line, i) => {
      drawText(page, fonts.regular, line, A4_WIDTH / 2, cardTop + 236 + i * 15, 11, MUTED, {
        align: 'center',
      })
    })
  } else {
    roundedRect(page, MARGIN, cardTop, cardW, cardH, 14, SURFACE_2)
    let angle = -Math.PI / 2
    const gap = slices.length > 1 ? 0.03 : 0
    for (const slice of slices) {
      const sweep = slice.share * Math.PI * 2
      const a1 = angle + Math.max(sweep - gap, 0.01)
      page.drawSvgPath(annularPath(cx, cy, 82, 58, angle, a1), {
        color: hexRgb(vizHex(slice.slot)),
      })
      angle += sweep
    }
    drawText(page, fonts.semibold, labels.totalExpenses.toUpperCase(), cx, cy - 6, 8, MUTED, {
      align: 'center',
      maxWidth: 88,
    })
    drawText(page, fonts.semibold, labels.totalAmount, cx, cy + 12, 15, FG, {
      align: 'center',
      maxWidth: 100,
    })

    const legendX = MARGIN + 290
    let ly = cardTop + 40
    const moneyLocale = getLocale() === 'el' ? 'el-GR' : 'en-US'
    for (const slice of slices) {
      roundedRect(page, legendX - 4, ly - 4, 8, 8, 2, vizHex(slice.slot))
      drawText(page, fonts.regular, slice.name, legendX + 14, ly + 4, 11, FG, {
        maxWidth: 130,
      })
      drawText(
        page,
        fonts.semibold,
        formatMoney(slice.amount, model.currency, moneyLocale),
        MARGIN + cardW - 72,
        ly + 4,
        11,
        FG,
        { align: 'right' },
      )
      drawText(
        page,
        fonts.regular,
        `${(slice.share * 100).toFixed(1)}%`,
        MARGIN + cardW - 16,
        ly + 4,
        10,
        MUTED,
        { align: 'right' },
      )
      ly += 28
    }
  }

  const barY = cardTop + cardH + 16
  roundedRect(page, MARGIN, barY, cardW, 44, 10, SURFACE)
  drawText(page, fonts.regular, labels.totalExpenses, MARGIN + 16, barY + 28, 12, empty ? MUTED : FG)
  drawText(page, fonts.semibold, labels.totalAmount, MARGIN + cardW - 16, barY + 28, 16, FG, {
    align: 'right',
  })
  if (!empty) {
    wrapLines(fonts.regular, labels.sliceNote, 9, cardW).forEach((line, i) => {
      drawText(page, fonts.regular, line, MARGIN, barY + 68 + i * 12, 9, MUTED)
    })
  }

  page.drawRectangle({
    x: MARGIN,
    y: 52,
    width: cardW,
    height: 1,
    color: hexRgb('#232c26'),
  })
  drawBrandMark(page, MARGIN, A4_HEIGHT - 38, 12)
  drawText(page, fonts.regular, labels.footerPrivacy, MARGIN + 18, A4_HEIGHT - 30, 9, MUTED)
  drawText(page, fonts.regular, labels.footerLocal, A4_WIDTH - MARGIN, A4_HEIGHT - 30, 9, MUTED, {
    align: 'right',
  })
}

/** Dark A4 monthly-expenses PDF bytes (vector). */
export async function buildExpensePdfBytes(input: ExpensePdfInput): Promise<Uint8Array> {
  const model = buildExpensePdfModel(input)
  const pdf = await PDFDocument.create()
  pdf.setTitle(`${model.labels.title} · ${model.entityName}`)
  pdf.setCreator('Oikonomia')
  pdf.setProducer('Oikonomia')
  const page = pdf.addPage([A4_WIDTH, A4_HEIGHT])
  const fonts = await embedReportFonts(pdf)
  paintPdfPage(page, model, fonts)
  return pdf.save()
}

/** Failures of the save step show the PDF sentence; any other code shows its own copy. */
export function pdfExportErrorMessage(err: unknown): string {
  const exportFailureCodes = [
    'save_location_invalid',
    'save_failed',
    'file_data_invalid',
    'file_too_large',
  ]
  const code = asCommandError(err).code

  if (exportFailureCodes.includes(code)) {
    // The sentence hides the cause; keep it for diagnosis.
    logCommandError(err)

    return t('reports.pdf.error')
  }

  return commandErrorMessage(err, 'reports.pdf.error')
}
