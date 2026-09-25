import { useEffect, useState } from 'react'

const REDUCED_MOTION = '(prefers-reduced-motion: reduce)'

function reducedMotionQuery(): MediaQueryList | null {
  // jsdom, and some embedded webviews, ship without matchMedia. Treat that as
  // "no preference" instead of crashing the shell.
  return typeof window.matchMedia === 'function' ? window.matchMedia(REDUCED_MOTION) : null
}

function readMotionAllowed(): boolean {
  const reduced = reducedMotionQuery()?.matches ?? false
  return !reduced && document.visibilityState === 'visible' && document.hasFocus()
}

/**
 * True while ambient motion may run: the user has not asked for reduced
 * motion, the page is visible and the window has focus. A background window
 * holds still, so the aurora never spends GPU time nobody is watching.
 */
export function useMotionAllowed(): boolean {
  const [allowed, setAllowed] = useState(readMotionAllowed)

  useEffect(() => {
    const update = () => setAllowed(readMotionAllowed())
    const query = reducedMotionQuery()

    query?.addEventListener('change', update)
    document.addEventListener('visibilitychange', update)
    window.addEventListener('focus', update)
    window.addEventListener('blur', update)

    return () => {
      query?.removeEventListener('change', update)
      document.removeEventListener('visibilitychange', update)
      window.removeEventListener('focus', update)
      window.removeEventListener('blur', update)
    }
  }, [])

  return allowed
}
