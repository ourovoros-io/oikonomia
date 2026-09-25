import { useMotionAllowed } from '../lib/motion'

/**
 * The ambient light behind every glass pane: five slow radial blobs and a
 * veil that keeps text sitting directly on the aurora legible. Purely
 * decorative, so it is hidden from assistive technology, and it holds still
 * whenever useMotionAllowed says nobody should pay for motion.
 *
 * `paused`, when true, holds it still regardless of focus: App mounts a
 * single Aurora above every status branch, and passes this while the vault
 * is locked so it does not keep animating unseen behind the opaque
 * UnlockScreen.
 */
export function Aurora({ paused = false }: { paused?: boolean }) {
  const moving = useMotionAllowed() && !paused

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
    </div>
  )
}
