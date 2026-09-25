import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type RefObject,
} from 'react'

import type { CashFlowSeries } from '../lib/api'
import { barPath, bucketAt, bucketDate, pulseLayout } from '../lib/cashFlowPulse'
import { cn } from '../lib/cn'
import { t } from '../lib/i18n'
import { useI18n } from '../lib/I18nProvider'
import { LEDGER_IN_STOPS, LEDGER_OUT_STOPS } from '../lib/ledgerColours'

/** Below this height the chart has no room for the peak labels. */
const LABEL_MIN_HEIGHT = 100
const LABEL_INSET = 16
const PLAIN_INSET = 4

const TOOLTIP_OFFSET = 14

type Size = { width: number; height: number }

/** Tracks the element's box; the chart is laid out in real CSS pixels. */
function useBoxSize(ref: RefObject<HTMLElement | null>): Size {
  const [size, setSize] = useState<Size>({ width: 0, height: 0 })

  useEffect(() => {
    const element = ref.current
    if (!element) return

    const measure = () => {
      const next = { width: element.clientWidth, height: element.clientHeight }
      setSize((previous) =>
        previous.width === next.width && previous.height === next.height ? previous : next,
      )
    }

    measure()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    return () => observer.disconnect()
  }, [ref])

  return size
}

/**
 * Money as a pulse: one slim bar per day (or month), money in rising above
 * the zero line and money out hanging below it, on one scale. Hover or use
 * the arrow keys to read a bucket. The amounts come from Rust; this component
 * only lays them out and paints them.
 */
export function CashFlowPulse({
  series,
  label,
  formatAmount,
  className = '',
}: {
  series: CashFlowSeries | null
  label: string
  formatAmount: (minor: number) => string
  className?: string
}) {
  const { locale } = useI18n()
  const rootRef = useRef<HTMLDivElement>(null)
  const tooltipRef = useRef<HTMLDivElement>(null)
  const { width, height } = useBoxSize(rootRef)
  const [active, setActive] = useState<number | null>(null)
  // useId returns characters that are not valid inside url(#...).
  const id = useId().replace(/[^a-zA-Z0-9_-]/g, '')

  const showLabels = height >= LABEL_MIN_HEIGHT
  const layout = useMemo(
    () => pulseLayout(series, { width, height, inset: showLabels ? LABEL_INSET : PLAIN_INSET }),
    [series, width, height, showLabels],
  )
  const buckets = series?.buckets ?? []
  const { zeroY, band, barWidth, bars } = layout

  const dateFormat = useMemo(
    () =>
      new Intl.DateTimeFormat(
        locale,
        series?.granularity === 'month'
          ? { month: 'long', year: 'numeric' }
          : { weekday: 'short', day: 'numeric', month: 'short' },
      ),
    [locale, series?.granularity],
  )

  // A new series can be shorter than the bucket being read.
  const reading = active !== null && active < bars.length ? active : null
  const activeBar = reading !== null ? bars[reading] : undefined
  const activeBucket = reading !== null ? buckets[reading] : undefined

  const [tooltipLeft, setTooltipLeft] = useState(0)
  useEffect(() => {
    if (!activeBar) return
    const tooltipWidth = tooltipRef.current?.offsetWidth ?? 0
    const right = activeBar.x + TOOLTIP_OFFSET
    setTooltipLeft(
      right + tooltipWidth <= width
        ? right
        : Math.max(0, activeBar.x - TOOLTIP_OFFSET - tooltipWidth),
    )
  }, [activeBar, width])

  const onPointerMove = (event: PointerEvent<SVGSVGElement>) => {
    const box = event.currentTarget.getBoundingClientRect()
    setActive(bucketAt(layout, event.clientX - box.left))
  }

  const onKeyDown = (event: KeyboardEvent<SVGSVGElement>) => {
    if (bars.length === 0) return
    if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
      event.preventDefault()
      const step = event.key === 'ArrowRight' ? 1 : -1
      setActive((current) => {
        if (current === null) return step > 0 ? 0 : bars.length - 1
        return Math.min(bars.length - 1, Math.max(0, current + step))
      })
    } else if (event.key === 'Escape') {
      setActive(null)
    }
  }

  const peakIn = layout.peakIn !== null ? bars[layout.peakIn] : undefined
  const peakOut = layout.peakOut !== null ? bars[layout.peakOut] : undefined
  const drawn = width > 0 && height > 0

  const barPaths = bars.flatMap((bar) => {
    const paths: { key: string; d: string; tone: 'in' | 'out' }[] = []
    if (bar.inHeight > 0) {
      paths.push({
        key: `in-${bar.index}`,
        d: barPath(bar.x, barWidth, zeroY, bar.inHeight, -1),
        tone: 'in',
      })
    }
    if (bar.outHeight > 0) {
      paths.push({
        key: `out-${bar.index}`,
        d: barPath(bar.x, barWidth, zeroY, bar.outHeight, 1),
        tone: 'out',
      })
    }
    return paths
  })
  const fillOf = (tone: 'in' | 'out') => `url(#pulse-${tone}-${id})`

  return (
    <div ref={rootRef} className={cn('relative', className)}>
      <svg
        role="img"
        aria-label={label}
        tabIndex={drawn && bars.length > 0 ? 0 : undefined}
        viewBox={drawn ? `0 0 ${width} ${height}` : undefined}
        className="absolute inset-0 size-full overflow-visible rounded-[10px] touch-pan-y"
        onPointerMove={onPointerMove}
        onPointerLeave={() => setActive(null)}
        onBlur={() => setActive(null)}
        onKeyDown={onKeyDown}
      >
        {drawn ? (
          <>
            <defs>
              <linearGradient id={`pulse-in-${id}`} x1="0" y1="0" x2="0" y2="1">
                <stop offset="0" stopColor={LEDGER_IN_STOPS[0]} />
                <stop offset="0.5" stopColor={LEDGER_IN_STOPS[1]} />
                <stop offset="1" stopColor={LEDGER_IN_STOPS[2]} stopOpacity="0.75" />
              </linearGradient>
              <linearGradient id={`pulse-out-${id}`} x1="0" y1="0" x2="0" y2="1">
                <stop offset="0" stopColor={LEDGER_OUT_STOPS[0]} stopOpacity="0.75" />
                <stop offset="0.5" stopColor={LEDGER_OUT_STOPS[1]} />
                <stop offset="1" stopColor={LEDGER_OUT_STOPS[2]} />
              </linearGradient>
              <filter id={`pulse-glow-${id}`} x="-100%" y="-20%" width="300%" height="140%">
                <feGaussianBlur stdDeviation="3" />
              </filter>
            </defs>

            {series?.granularity === 'day' && band >= 4
              ? buckets.map((bucket, index) =>
                  bucketDate(bucket.start).getDay() === 6 ? (
                    <rect
                      key={`weekend-${bucket.start}`}
                      data-pulse-weekend
                      x={layout.left + band * index}
                      y={0}
                      width={band * Math.min(2, buckets.length - index)}
                      height={height}
                      rx={6}
                      fill="rgba(255,255,255,0.025)"
                    />
                  ) : null,
                )
              : null}

            <line x1={0} x2={width} y1={zeroY} y2={zeroY} stroke="rgba(255,255,255,0.16)" />

            {activeBar ? (
              <rect
                data-pulse-cursor
                x={activeBar.x - band / 2}
                y={0}
                width={band}
                height={height}
                rx={Math.min(6, band / 2)}
                fill="rgba(255,255,255,0.07)"
              />
            ) : null}

            <g filter={`url(#pulse-glow-${id})`} opacity={0.5} aria-hidden="true">
              {barPaths.map((bar) => (
                <path
                  key={bar.key}
                  d={bar.d}
                  fill={fillOf(bar.tone)}
                  className={`pulse-bar pulse-bar-${bar.tone}`}
                />
              ))}
            </g>
            <g data-pulse-bars>
              {barPaths.map((bar) => (
                <path
                  key={bar.key}
                  data-pulse-bar={bar.tone}
                  d={bar.d}
                  fill={fillOf(bar.tone)}
                  className={`pulse-bar pulse-bar-${bar.tone}`}
                />
              ))}
            </g>

            {showLabels && peakIn && layout.peakIn !== null ? (
              <text
                x={peakIn.x + barWidth / 2 + 5}
                y={zeroY - peakIn.inHeight + 9}
                className="font-mono text-[11px] tabular-nums"
                fill="var(--color-money-in-text)"
              >
                {formatAmount(buckets[layout.peakIn]?.income_minor ?? 0)}
              </text>
            ) : null}
            {showLabels && peakOut && layout.peakOut !== null ? (
              <text
                x={peakOut.x + barWidth / 2 + 5}
                y={zeroY + peakOut.outHeight - 2}
                className="font-mono text-[11px] tabular-nums"
                fill="var(--color-money-out-text)"
              >
                {formatAmount(buckets[layout.peakOut]?.expenses_minor ?? 0)}
              </text>
            ) : null}
          </>
        ) : null}
      </svg>

      <div
        ref={tooltipRef}
        role="status"
        aria-live="polite"
        className={cn(
          'pointer-events-none absolute top-1/2 left-0 z-10 min-w-36 -translate-y-1/2 rounded-[10px] border border-[var(--color-border-strong)] bg-[rgba(28,33,43,0.92)] px-2.5 py-2 text-[12.5px] tabular-nums shadow-[0_10px_30px_rgba(0,0,0,0.4)] transition-opacity duration-100',
          activeBucket ? 'opacity-100' : 'opacity-0',
        )}
        style={{ left: tooltipLeft }}
      >
        {activeBucket ? (
          <>
            <p className="mb-1 font-mono text-[10.5px] tracking-[0.08em] text-[var(--color-muted)] uppercase">
              {dateFormat.format(bucketDate(activeBucket.start))}
            </p>
            <p className="flex justify-between gap-4">
              <span className="text-[var(--color-fg-secondary)]">{t('dashboard.pill.in')}</span>
              <b className="font-semibold text-[var(--color-money-in-text)]">
                {activeBucket.income_minor > 0 ? formatAmount(activeBucket.income_minor) : '—'}
              </b>
            </p>
            <p className="flex justify-between gap-4">
              <span className="text-[var(--color-fg-secondary)]">{t('dashboard.pill.out')}</span>
              <b className="font-semibold text-[var(--color-money-out-text)]">
                {activeBucket.expenses_minor > 0 ? formatAmount(activeBucket.expenses_minor) : '—'}
              </b>
            </p>
          </>
        ) : null}
      </div>
    </div>
  )
}
