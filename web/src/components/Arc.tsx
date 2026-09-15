import { useId } from 'react'

import { ARC_START_DEG, ARC_SWEEP_DEG, arcEndDeg, arcFraction, arcPath, arcPoint, formatPercentFromBps } from '../lib/arc'
import { LEDGER_IN_STOPS, LEDGER_OUT_STOPS } from '../lib/cashFlowLight'
import { cn } from '../lib/cn'

export type ArcTone = 'in' | 'out'

const CENTRE = 35
const RADIUS = 26

/**
 * The 270-degree meter: a quiet track, a Ledger gradient stroke over a blurred
 * bloom copy of itself, and a lit pointer dot at the value.
 */
export function Arc({ fraction, tone, className = '' }: { fraction: number | null; tone: ArcTone; className?: string }) {
  // useId returns characters that are not valid inside url(#...).
  const id = useId().replace(/[^a-zA-Z0-9_-]/g, '')
  const stops = tone === 'in' ? LEDGER_IN_STOPS : LEDGER_OUT_STOPS
  const lit = fraction !== null && fraction > 0
  const end = arcEndDeg(fraction ?? 0)
  const pointer = arcPoint(CENTRE, CENTRE, RADIUS, end)
  const valuePath = arcPath(CENTRE, CENTRE, RADIUS, ARC_START_DEG, end)

  return (
    <svg
      viewBox="0 0 70 70"
      aria-hidden="true"
      data-arc-tone={tone}
      className={cn('size-[76px] shrink-0 overflow-visible', className)}
    >
      <defs>
        <linearGradient id={`arc-gradient-${id}`} x1="0" y1="1" x2="1" y2="0">
          <stop offset="0" stopColor={stops[0]} />
          <stop offset="0.55" stopColor={stops[1]} />
          <stop offset="1" stopColor={stops[2]} />
        </linearGradient>
        <filter id={`arc-bloom-${id}`} x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation="4.5" />
        </filter>
      </defs>
      <path
        d={arcPath(CENTRE, CENTRE, RADIUS, ARC_START_DEG, ARC_START_DEG + ARC_SWEEP_DEG)}
        fill="none"
        stroke="rgba(255,255,255,0.1)"
        strokeWidth="3"
        strokeLinecap="round"
      />
      {lit ? (
        <g data-arc-value>
          <path d={valuePath} fill="none" stroke={`url(#arc-gradient-${id})`} strokeWidth="6" strokeLinecap="round" filter={`url(#arc-bloom-${id})`} opacity="0.85" />
          <path d={valuePath} fill="none" stroke={`url(#arc-gradient-${id})`} strokeWidth="3.2" strokeLinecap="round" />
          <circle cx={pointer.x} cy={pointer.y} r="5" fill={stops[1]} filter={`url(#arc-bloom-${id})`} />
          <circle cx={pointer.x} cy={pointer.y} r="2.6" fill="#f4f7fb" />
        </g>
      ) : null}
      <circle cx={CENTRE} cy={CENTRE} r="1.4" fill="#5c6472" />
    </svg>
  )
}

/** A glass tile: an arc, its percent, a label and a hint. */
export function ArcTile({
  label,
  hint,
  bps,
  tone,
  locale,
  noValueLabel,
  signed = false,
  fullScaleBps = 10_000,
}: {
  label: string
  hint?: string
  bps: number | null
  tone: ArcTone
  locale: string
  noValueLabel: string
  signed?: boolean
  fullScaleBps?: number
}) {
  const hintId = useId()
  const fraction = arcFraction(bps === null ? null : signed ? Math.abs(bps) : bps, fullScaleBps)
  const value = bps === null ? null : formatPercentFromBps(bps, locale, { signed })

  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}
      aria-valuetext={value === null ? noValueLabel : `${value}%`}
      aria-describedby={hint ? hintId : undefined}
      className="glass-pane grid grid-cols-[76px_minmax(0,1fr)] items-center gap-3.5 rounded-[20px] px-4.5 py-4"
    >
      <Arc fraction={fraction} tone={tone} />
      <div className="min-w-0">
        <div aria-hidden="true" className="text-[26px] leading-none font-semibold tabular-nums text-[var(--color-fg)]">
          {value === null ? (
            '—'
          ) : (
            <>
              {value}
              <small className="ml-0.5 font-mono text-[11px] font-medium text-[var(--color-muted)]">%</small>
            </>
          )}
        </div>
        <div aria-hidden="true" className="mt-1.5 truncate text-sm font-medium text-[var(--color-fg)]">
          {label}
        </div>
        {hint ? (
          <div id={hintId} className="mt-0.5 truncate text-[12.5px] text-[var(--color-muted)]">
            {hint}
          </div>
        ) : null}
      </div>
    </div>
  )
}
