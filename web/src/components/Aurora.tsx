import { useMotionAllowed } from '../lib/motion'

/**
 * The ambient light behind every glass pane: five slow radial blobs, a veil
 * that keeps text sitting directly on the aurora legible, and the Greek
 * wordmark. Purely decorative, so it is hidden from assistive technology, and
 * it holds still whenever useMotionAllowed says nobody should pay for motion.
 */
export function Aurora({ watermark = true }: { watermark?: boolean }) {
  const moving = useMotionAllowed()

  return (
    <div className="aurora" data-moving={moving} aria-hidden="true">
      <div className="aurora-blobs">
        <i />
        <i />
        <i />
        <i />
        <i />
      </div>

      <div className="aurora-veil" />

      {watermark ? <div className="aurora-watermark">ΟΙΚΟΝΟΜΙΑ</div> : null}
    </div>
  )
}
