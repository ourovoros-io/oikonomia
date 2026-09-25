import { useState } from 'react'
import { formatMoney, type ReportLine } from '../lib/api'
import { formatPercentFromBps } from '../lib/arc'
import { buildSlices, vizVar } from '../lib/expenseSlices'
import { localeForCurrency } from '../lib/money'
import { cn } from '../lib/cn'
import { t } from '../lib/i18n'
import { useI18n } from '../lib/I18nProvider'

const SIZE = 180
const RADIUS = 70
const STROKE = 24
const CIRCUMFERENCE = 2 * Math.PI * RADIUS
const GAP = 2

/** Donut of period expenses by category, with a hover readout in the hole. */
export function ExpenseDonut({ lines, ccy }: { lines: ReportLine[]; ccy: string }) {
  useI18n()
  const [hover, setHover] = useState<number | null>(null)
  const { slices, total } = buildSlices(lines)
  // Percentages use the money's locale, so "29,5 %" sits beside "720,00 €".
  const percent = (share: number) => `${formatPercentFromBps(Math.round(share * 10_000), localeForCurrency(ccy))}%`

  if (slices.length === 0) {
    return (
      <div className="px-5 py-10 text-center text-sm text-[var(--color-muted)]">
        {t('donut.noExpenses')}
      </div>
    )
  }

  const active = hover !== null ? slices[hover] : null
  const gap = slices.length > 1 ? GAP : 0

  let acc = 0
  const segments = slices.map((slice) => {
    const start = acc
    const full = slice.share * CIRCUMFERENCE
    acc += full
    return { start, length: Math.max(full - gap, 0.5) }
  })

  return (
    <div className="flex flex-wrap items-center justify-center gap-x-10 gap-y-6 px-5 py-6">
      <div className="relative" onMouseLeave={() => setHover(null)}>
        <svg width={SIZE} height={SIZE} viewBox={`0 0 ${SIZE} ${SIZE}`} aria-hidden="true">
          <g transform={`rotate(-90 ${SIZE / 2} ${SIZE / 2})`}>
            {segments.map((seg, i) => (
              <circle
                key={slices[i].name}
                cx={SIZE / 2}
                cy={SIZE / 2}
                r={RADIUS}
                fill="none"
                stroke={vizVar(slices[i].slot)}
                strokeWidth={hover === i ? STROKE + 4 : STROKE}
                strokeDasharray={`${seg.length} ${CIRCUMFERENCE - seg.length}`}
                strokeDashoffset={-seg.start}
                opacity={hover === null || hover === i ? 1 : 0.35}
                className="oik-donut-seg transition-[stroke-width,opacity] duration-150"
                style={{
                  // Stagger by start angle so the ring reads as one clockwise sweep.
                  animationDelay: `${Math.round((seg.start / CIRCUMFERENCE) * 250)}ms`,
                }}
                onMouseEnter={() => setHover(i)}
              />
            ))}
          </g>
        </svg>
        <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center px-9 text-center">
          <span className="w-full truncate font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
            {active ? active.name : t('reports.pdf.totalExpenses')}
          </span>
          <span className="mt-1 w-full truncate text-xl font-semibold tabular-nums text-[var(--color-fg)]">
            {formatMoney(active ? active.amount : total, ccy)}
          </span>
          {active ? (
            <span className="text-xs tabular-nums text-[var(--color-muted)]">
              {percent(active.share)}
            </span>
          ) : null}
        </div>
      </div>

      <ul className="min-w-0 flex-1 basis-64 space-y-1" onMouseLeave={() => setHover(null)}>
        {slices.map((slice, i) => (
          <li
            key={slice.name}
            onMouseEnter={() => setHover(i)}
            className={cn(
              'flex items-center gap-2.5 rounded-lg px-2 py-1.5 transition',
              hover === i && 'bg-[var(--color-surface-2)]/70',
            )}
          >
            <span
              className="size-2.5 shrink-0 rounded-sm"
              style={{ background: vizVar(slice.slot) }}
              aria-hidden
            />
            <span className="min-w-0 flex-1 truncate text-sm text-[var(--color-fg-secondary)]">
              {slice.name}
            </span>
            <span className="shrink-0 text-sm tabular-nums text-[var(--color-fg)]">
              {formatMoney(slice.amount, ccy)}
            </span>
            <span className="w-12 shrink-0 text-right text-xs tabular-nums text-[var(--color-muted)]">
              {percent(slice.share)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  )
}
