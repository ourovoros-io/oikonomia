/**
 * The ambient light behind every glass pane: five static radial blobs and a
 * veil that keeps text sitting directly on the aurora legible. Purely
 * decorative, so it is hidden from assistive technology. It does not animate:
 * motion under the panes' backdrop-filter is too costly without a GPU.
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
