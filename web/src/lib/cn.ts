/**
 * Join truthy class names. Lives outside ui.tsx so that module exports only
 * React components — a requirement for Vite's Fast Refresh to hot-swap it.
 */
export function cn(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(' ')
}
