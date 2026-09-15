import { useContext, useLayoutEffect, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

import { TopBarContext } from '../lib/topBar'

/**
 * A page's title, subtitle and actions, drawn in the app header. Outside the
 * app shell (page tests, isolated renders) it renders inline instead.
 */
export function TopBar({
  title,
  subtitle,
  actions,
}: {
  title: string
  subtitle?: string
  actions?: ReactNode
}) {
  const slots = useContext(TopBarContext)
  const claimTitle = slots?.claimTitle

  // Claim before paint: a passive effect would let the fallback book title
  // show beside this page's own title for a frame on every mount/navigation.
  useLayoutEffect(() => claimTitle?.(), [claimTitle])

  const heading = (
    <>
      <h1 className="truncate text-xl font-semibold tracking-tight text-[var(--color-fg)]">{title}</h1>
      {subtitle ? (
        <span className="truncate text-sm text-[var(--color-fg-secondary)]">{subtitle}</span>
      ) : null}
    </>
  )

  if (!slots) {
    return (
      <div className="mb-1 flex flex-wrap items-center justify-between gap-4">
        <div className="flex min-w-0 items-baseline gap-2.5">{heading}</div>
        {actions ? <div className="flex flex-wrap items-center gap-2.5">{actions}</div> : null}
      </div>
    )
  }

  return (
    <>
      {slots.title ? createPortal(heading, slots.title) : null}
      {actions && slots.actions ? createPortal(actions, slots.actions) : null}
    </>
  )
}
