import { useId } from 'react'

type Props = { className?: string }

const SHIELD_PATH =
  'M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1 1 0 0 1 1.52 0C14.5 3.8 17 5 19 5a1 1 0 0 1 1 1z'

/**
 * Brand mark: a shield (encryption) wrapped around the oikos — pediment and
 * two columns with a keyhole doorway, a euro at the vault door. Master art
 * lives in docs/assets/logo.svg; keep the two in sync.
 */
export function Logo({ className }: Props) {
  const id = useId()

  const mark = `mark-${id}`
  const plate = `plate-${id}`
  const glow = `glow-${id}`

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
      </defs>

      <rect width="512" height="512" rx="116" fill={`url(#${plate})`} />
      <rect width="512" height="512" rx="116" fill={`url(#${glow})`} />

      <g transform="translate(256 256) scale(0.78) translate(-256 -256)">
        <g transform="translate(-20 -23.4) scale(23)">
          <path
            fill="none"
            stroke={`url(#${mark})`}
            strokeWidth="1.2"
            strokeLinecap="round"
            strokeLinejoin="round"
            d={SHIELD_PATH}
          />
        </g>

        <g transform="translate(256 240) scale(0.56) translate(-256 -238)">
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
          <path
            fill="none"
            stroke={`url(#${mark})`}
            strokeWidth="11.5"
            strokeLinecap="round"
            d="M 273 297.7 A 25 25 0 1 0 273 334.3 M 226 310.2 H 258 M 226 321.8 H 258"
          />
        </g>
      </g>
    </svg>
  )
}
