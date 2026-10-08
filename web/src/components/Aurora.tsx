/**
 * The ambient light behind every glass pane: five static radial blobs and a
 * veil that keeps text sitting directly on the aurora legible. Purely
 * decorative, so it is hidden from assistive technology. It does not animate,
 * and the panes above it do not blur it again: both cost too much CPU on
 * machines without a GPU.
 */
export function Aurora() {
  return (
    <div className="aurora" aria-hidden="true">
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
