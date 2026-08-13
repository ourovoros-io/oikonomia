/** Claim a submit/in-flight slot. Returns false if already claimed. */
export function beginExclusive(busyRef: { current: boolean }): boolean {
  if (busyRef.current) return false
  busyRef.current = true
  return true
}
