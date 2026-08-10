import { useId } from 'react'

type Props = { className?: string }

const SHIELD_PATH =
  'M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1 1 0 0 1 1.52 0C14.5 3.8 17 5 19 5a1 1 0 0 1 1 1z'

/**
 * Brand mark: a Greek temple pediment over two columns (double-entry) with a
 * keyhole doorway (the encrypted vault), guarded by a shield badge carrying
 * the euro. Master art lives in docs/assets/logo.svg; keep the two in sync.
 */
export function Logo({ className }: Props) {
  const id = useId()

  const mark = `mark-${id}`
  const plate = `plate-${id}`
  const glow = `glow-${id}`
  const houseGap = `house-gap-${id}`
  const euroCut = `euro-cut-${id}`

  return (
    <svg viewBox="0 0 512 512" className={className} role="img" aria-label="Oikonomia">
      <defs>
        <linearGradient id={mark} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#9d97ff" />
          <stop offset="1" stopColor="#5a51f0" />
        </linearGradient>
        <linearGradient id={plate} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#17171d" />
          <stop offset="1" stopColor="#09090b" />
        </linearGradient>
        <radialGradient id={glow} cx="0.5" cy="0.16" r="0.75">
          <stop offset="0" stopColor="#635bff" stopOpacity="0.28" />
          <stop offset="1" stopColor="#635bff" stopOpacity="0" />
        </radialGradient>

        <mask id={houseGap}>
          <rect width="512" height="512" fill="white" />
          <g transform="translate(306 300) scale(8)">
            <path
              fill="black"
              stroke="black"
              strokeWidth="4"
              strokeLinejoin="round"
              d={SHIELD_PATH}
            />
          </g>
        </mask>

        <mask id={euroCut}>
          <rect width="512" height="512" fill="white" />
          <g
            transform="translate(306 300) scale(8)"
            fill="none"
            stroke="black"
            strokeWidth="1.7"
            strokeLinecap="round"
          >
            <path d="M 14.87 9.79 A 3.6 3.6 0 1 0 14.87 14.61" />
            <path d="M 7.7 11.35 H 12.3" />
            <path d="M 7.7 13.05 H 12.3" />
          </g>
        </mask>
      </defs>

      <rect width="512" height="512" rx="116" fill={`url(#${plate})`} />
      <rect width="512" height="512" rx="116" fill={`url(#${glow})`} />

      <g transform="translate(256 256) scale(0.68) translate(-267 -270)">
        <g mask={`url(#${houseGap})`}>
          <path
            d="M 92 196 L 256 88 L 420 196"
            fill="none"
            stroke={`url(#${mark})`}
            strokeWidth="46"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <path
            fillRule="evenodd"
            fill={`url(#${mark})`}
            d="M 152 232 L 360 232 Q 388 232 388 260 L 388 398 Q 388 412 374 412 L 286 412 L 286 352 Q 298 336 298 316 A 42 42 0 1 0 214 316 Q 214 336 226 352 L 226 412 L 138 412 Q 124 412 124 398 L 124 260 Q 124 232 152 232 Z"
          />
          <rect x="100" y="436" width="312" height="44" rx="22" fill={`url(#${mark})`} />
        </g>

        <g mask={`url(#${euroCut})`}>
          <g transform="translate(306 300) scale(8)">
            <path fill={`url(#${mark})`} d={SHIELD_PATH} />
          </g>
        </g>
      </g>
    </svg>
  )
}
