import { useEffect, useRef, useState } from 'react'

import type { CashFlowSeries } from '../lib/api'
import {
  LEDGER_IN_STOPS,
  LEDGER_OUT_STOPS,
  ledgerColourAt,
  lightGeometry,
  type LedgerStops,
  type LightGeometry,
  type LightPoint,
} from '../lib/cashFlowLight'
import { cn } from '../lib/cn'
import { useMotionAllowed } from '../lib/motion'

/** Redraw at most about 30 times a second while breathing. */
const FRAME_MS = 33

type Layer = 'glow' | 'sharp'

function traceBand(ctx: CanvasRenderingContext2D, edge: LightPoint[], zeroY: number) {
  const first = edge[0]
  const last = edge[edge.length - 1]
  if (!first || !last) return
  ctx.beginPath()
  ctx.moveTo(first.x, zeroY)
  for (const point of edge) ctx.lineTo(point.x, point.y)
  ctx.lineTo(last.x, zeroY)
  ctx.closePath()
}

function fillBand(
  ctx: CanvasRenderingContext2D,
  edge: LightPoint[],
  zeroY: number,
  width: number,
  stops: LedgerStops,
  alpha: number,
) {
  const gradient = ctx.createLinearGradient(0, 0, width, 0)
  for (const offset of [0, 0.55, 1]) gradient.addColorStop(offset, ledgerColourAt(stops, offset, alpha))
  traceBand(ctx, edge, zeroY)
  ctx.fillStyle = gradient
  ctx.fill()
}

/** A soft white sheen, brightest at the band's far edge, fading to the line. */
function sheen(ctx: CanvasRenderingContext2D, edge: LightPoint[], zeroY: number, rising: boolean) {
  if (edge.length === 0) return
  const ys = edge.map((point) => point.y)
  const far = rising ? Math.min(...ys) : Math.max(...ys)
  if (far === zeroY) return
  const gradient = ctx.createLinearGradient(0, far, 0, zeroY)
  gradient.addColorStop(0, 'rgba(255,255,255,0.2)')
  gradient.addColorStop(0.35, 'rgba(255,255,255,0.05)')
  gradient.addColorStop(1, 'rgba(255,255,255,0)')
  ctx.save()
  ctx.globalCompositeOperation = 'lighter'
  traceBand(ctx, edge, zeroY)
  ctx.fillStyle = gradient
  ctx.fill()
  ctx.restore()
}

/** The bright edge: a glowing gradient stroke with a fine white core. */
function ridge(ctx: CanvasRenderingContext2D, edge: LightPoint[], width: number, stops: LedgerStops) {
  const first = edge[0]
  if (!first) return
  const gradient = ctx.createLinearGradient(0, 0, width, 0)
  gradient.addColorStop(0, ledgerColourAt(stops, 0, 0.95))
  gradient.addColorStop(1, ledgerColourAt(stops, 1, 0.95))
  ctx.save()
  ctx.shadowColor = ledgerColourAt(stops, 0.5, 0.9)
  ctx.shadowBlur = 10
  ctx.beginPath()
  ctx.moveTo(first.x, first.y)
  for (const point of edge.slice(1)) ctx.lineTo(point.x, point.y)
  ctx.strokeStyle = gradient
  ctx.lineWidth = 1.6
  ctx.stroke()
  ctx.shadowBlur = 0
  ctx.strokeStyle = 'rgba(255,255,255,0.55)'
  ctx.lineWidth = 0.6
  ctx.stroke()
  ctx.restore()
}

function paint(
  canvas: HTMLCanvasElement,
  layer: Layer,
  geometry: LightGeometry,
  width: number,
  height: number,
) {
  const ctx = canvas.getContext('2d')
  if (!ctx) return

  const dpr = Math.min(2, window.devicePixelRatio || 1)
  const pixelWidth = Math.round(width * dpr)
  const pixelHeight = Math.round(height * dpr)
  if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
    canvas.width = pixelWidth
    canvas.height = pixelHeight
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  ctx.clearRect(0, 0, width, height)

  const { zeroY, inEdge, outEdge } = geometry

  if (layer === 'glow') {
    fillBand(ctx, inEdge, zeroY, width, LEDGER_IN_STOPS, 0.8)
    fillBand(ctx, outEdge, zeroY, width, LEDGER_OUT_STOPS, 0.8)
    return
  }

  fillBand(ctx, inEdge, zeroY, width, LEDGER_IN_STOPS, 0.34)
  sheen(ctx, inEdge, zeroY, true)
  fillBand(ctx, outEdge, zeroY, width, LEDGER_OUT_STOPS, 0.3)
  sheen(ctx, outEdge, zeroY, false)

  ctx.strokeStyle = 'rgba(255,255,255,0.14)'
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.moveTo(0, Math.round(zeroY) + 0.5)
  ctx.lineTo(width, Math.round(zeroY) + 0.5)
  ctx.stroke()

  ridge(ctx, inEdge, width, LEDGER_IN_STOPS)
  ridge(ctx, outEdge, width, LEDGER_OUT_STOPS)
}

/**
 * Money drawn as light: in rises above a zero line, out hangs below it, on one
 * scale. It breathes gently only while motion is allowed and it is on screen;
 * otherwise it is painted once and held still. The numbers it draws come from
 * Rust; this component only paints them.
 */
export function CashFlowLight({
  series,
  label,
  className = '',
}: {
  series: CashFlowSeries | null
  label: string
  className?: string
}) {
  const rootRef = useRef<HTMLDivElement>(null)
  const glowRef = useRef<HTMLCanvasElement>(null)
  const sharpRef = useRef<HTMLCanvasElement>(null)
  const motionAllowed = useMotionAllowed()
  const [onScreen, setOnScreen] = useState(true)

  useEffect(() => {
    const root = rootRef.current
    if (!root || typeof IntersectionObserver === 'undefined') return
    const observer = new IntersectionObserver((entries) => {
      const latest = entries[entries.length - 1]
      if (latest) setOnScreen(latest.isIntersecting)
    })
    observer.observe(root)
    return () => observer.disconnect()
  }, [])

  const breathing = motionAllowed && onScreen

  useEffect(() => {
    const root = rootRef.current
    // Both canvases share one size; the sharp canvas is the size reference so
    // the geometry — the expensive part — is computed once per draw, not once
    // per layer.
    const draw = (time: number, breathe: number) => {
      const sharp = sharpRef.current
      if (!sharp) return
      const width = sharp.clientWidth
      const height = sharp.clientHeight
      if (width === 0 || height === 0) return
      const geometry = lightGeometry(series, { width, height, time, breathe })
      const glow = glowRef.current
      if (glow) paint(glow, 'glow', geometry, width, height)
      paint(sharp, 'sharp', geometry, width, height)
    }

    if (!breathing || typeof requestAnimationFrame !== 'function') {
      // At rest: paint once, and again only when the box changes size.
      draw(0, 0)
      if (!root || typeof ResizeObserver === 'undefined') return
      const observer = new ResizeObserver(() => draw(0, 0))
      observer.observe(root)
      return () => observer.disconnect()
    }

    let frame = 0
    let last = Number.NEGATIVE_INFINITY
    const tick = (now: number) => {
      frame = requestAnimationFrame(tick)
      if (now - last < FRAME_MS) return
      last = now
      draw(now / 1000, 1)
    }
    // Seed with the real clock, not 0: the first tick's `now / 1000` would
    // otherwise be a large jump from a standing start, jolting the wobble.
    draw(performance.now() / 1000, 1)
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [series, breathing])

  return (
    <div ref={rootRef} role="img" aria-label={label} className={cn('pointer-events-none relative', className)}>
      <canvas ref={glowRef} aria-hidden="true" className="cash-flow-glow absolute inset-0 size-full" />
      <canvas ref={sharpRef} aria-hidden="true" className="absolute inset-0 size-full" />
    </div>
  )
}
