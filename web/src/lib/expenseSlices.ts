import type { ReportLine } from './api'
import { t } from './i18n'

/**
 * Slices beyond this fold into a neutral "Other" — more hues would stop being
 * tellable apart (the palette's slot order is validated for adjacency).
 */
export const MAX_SLICES = 6

/** Dark-theme `--viz-*` hex from `web/src/index.css`. PDF always embeds these. */
export const VIZ_DARK_HEX = {
  1: '#3987e5',
  2: '#d95926',
  3: '#199e70',
  4: '#c98500',
  5: '#d55181',
  6: '#008300',
  other: '#4a554c',
} as const

export type VizSlot = 1 | 2 | 3 | 4 | 5 | 6 | 'other'

export type ExpenseSlice = {
  name: string
  amount: number
  share: number
  slot: VizSlot
}

export function vizVar(slot: VizSlot): string {
  return slot === 'other' ? 'var(--viz-other)' : `var(--viz-${slot})`
}

export function vizHex(slot: VizSlot): string {
  return slot === 'other' ? VIZ_DARK_HEX.other : VIZ_DARK_HEX[slot]
}

/**
 * Period expenses → donut / PDF slices. Top 6 by amount; those six take
 * viz-1..6 in stable account-code order so a category keeps its color when
 * rank changes. Anything past MAX_SLICES folds into Other / viz-other.
 */
export function buildSlices(lines: ReportLine[]): { slices: ExpenseSlice[]; total: number } {
  const positive = lines.filter((l) => l.balance_minor > 0)
  const total = positive.reduce((sum, l) => sum + l.balance_minor, 0)
  if (total <= 0) return { slices: [], total: 0 }

  const sorted = [...positive].sort((a, b) => b.balance_minor - a.balance_minor)
  const top = sorted.slice(0, MAX_SLICES)
  const rest = sorted.slice(MAX_SLICES)

  const byCode = [...top].sort((a, b) => a.code.localeCompare(b.code))
  const slotOf = new Map<string, VizSlot>(
    byCode.map((l, i) => [l.code, (i + 1) as VizSlot]),
  )

  const slices: ExpenseSlice[] = top.map((l) => ({
    name: l.name,
    amount: l.balance_minor,
    share: l.balance_minor / total,
    slot: slotOf.get(l.code) ?? 'other',
  }))

  if (rest.length > 0) {
    const other = rest.reduce((sum, l) => sum + l.balance_minor, 0)
    slices.push({
      name: t('reports.pdf.other', { n: rest.length }),
      amount: other,
      share: other / total,
      slot: 'other',
    })
  }

  return { slices, total }
}
