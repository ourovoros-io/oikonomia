/**
 * The chassis decor: two diagonal bands, three hairlines and two dot grids,
 * painted behind everything at the one suite angle (a rise of 600 over a run
 * of 320, about 62 degrees). Same construction as `decor.rs` in the plugin
 * faceplate kit; one angle reads as a system, three read as decoration.
 *
 * Fixed to the window rather than to the scrolling page, so the bands stay
 * put while content moves over them — the way the desk behind a plugin
 * editor does not move. `slice` scales uniformly, so the angle survives
 * every window shape.
 */
export function Decor() {
  return (
    <div className="pointer-events-none fixed inset-0 z-0 overflow-hidden" aria-hidden="true">
      <svg className="block h-full w-full" viewBox="0 0 1440 900" preserveAspectRatio="xMidYMid slice">
        <polygon points="183,900 663,0 870,0 390,900" fill="var(--color-surface-2)" opacity="0.55" />
        <polygon points="878,900 1358,0 1421,0 941,900" fill="var(--color-surface-2)" opacity="0.4" />
        <line x1="73" y1="900" x2="553" y2="0" stroke="var(--color-border)" strokeWidth="1" />
        <line x1="1074" y1="900" x2="1554" y2="0" stroke="var(--color-border)" strokeWidth="1" />
        <line
          x1="1232"
          y1="900"
          x2="1488"
          y2="420"
          stroke="var(--color-hazard)"
          strokeWidth="1"
          opacity="0.35"
        />
      </svg>
      <div className="dots absolute top-[74px] right-10 h-12 w-12" />
      <div className="dots absolute bottom-[62px] left-6 h-6 w-16" />
    </div>
  )
}
