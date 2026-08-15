import en from '../locales/en.json' with { type: 'json' }
import el from '../locales/el.json' with { type: 'json' }

/** Optimistic first-paint cache only. Durable store is UiPrefs.locale. */
export const LOCALE_STORAGE_KEY = 'oikonomia.locale'

export type Locale = 'en' | 'el'
export type MessageKey = keyof typeof en
export type TranslateVars = Record<string, string | number>

const catalogs: Record<Locale, Record<string, string>> = {
  en: en as Record<string, string>,
  el: el as Record<string, string>,
}

let current: Locale = 'en'
const listeners = new Set<() => void>()
let testMessages: Partial<Record<Locale, Record<string, string>>> = {}

export function parseLocale(value: unknown): Locale {
  return value === 'el' || value === 'en' ? value : 'en'
}

export function getLocale(): Locale {
  return current
}

export function subscribeLocale(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function notify(): void {
  for (const listener of listeners) listener()
}

function writeCache(locale: Locale): void {
  try {
    localStorage.setItem(LOCALE_STORAGE_KEY, locale)
  } catch {
    /* private mode / quota */
  }
}

/** Read the optimistic cache. Invalid values become `en`. Missing → null. */
export function readCachedLocale(): Locale | null {
  try {
    const raw = localStorage.getItem(LOCALE_STORAGE_KEY)
    if (raw === null) return null
    return parseLocale(raw)
  } catch {
    return null
  }
}

function lookup(locale: Locale, key: string): string | undefined {
  const override = testMessages[locale]?.[key]
  if (typeof override === 'string' && override.trim() !== '') return override
  const value = catalogs[locale]?.[key]
  if (typeof value === 'string' && value.trim() !== '') return value
  return undefined
}

function interpolate(template: string, vars?: TranslateVars): string {
  if (!vars) return template
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    vars[name] === undefined ? match : String(vars[name]),
  )
}

/** Look up `key`. Empty/missing `el` values fall back to English. */
export function t(key: string, vars?: TranslateVars): string {
  const raw = lookup(current, key) ?? lookup('en', key) ?? key
  return interpolate(raw, vars)
}

/** Apply locale in memory + optimistic cache. Does not persist to UiPrefs. */
export function applyLocale(locale: Locale): void {
  const next = parseLocale(locale)
  if (next === current) {
    writeCache(next)
    return
  }
  current = next
  writeCache(next)
  notify()
}

/** Apply `prefs.locale` (absent/invalid → `en`). */
export function applyLocaleFromPrefs(prefs: { locale?: unknown } | null | undefined): Locale {
  const locale = parseLocale(prefs?.locale)
  applyLocale(locale)
  return locale
}

type PersistFn = (locale: Locale) => void | Promise<void>
let persistLocale: PersistFn | null = null

/** Wire durable persist (UiPrefs / settings_set_locale). Tests inject a mock. */
export function setLocalePersist(fn: PersistFn | null): void {
  persistLocale = fn
}

/**
 * Programmatic locale change. Updates memory + optimistic cache, then persists
 * via the wired UiPrefs writer when one is configured.
 */
export function setLocale(locale: Locale): void {
  applyLocale(locale)
  void persistLocale?.(locale)
}

/** Test-only catalog overlay so el.json can stay empty for Writer. */
export function setLocaleMessagesForTests(
  locale: Locale,
  messages: Record<string, string> | null,
): void {
  if (messages === null) {
    delete testMessages[locale]
  } else {
    testMessages[locale] = messages
  }
  notify()
}

export function resetI18nForTests(): void {
  current = 'en'
  testMessages = {}
  persistLocale = null
  try {
    localStorage.removeItem(LOCALE_STORAGE_KEY)
  } catch {
    /* ignore */
  }
  notify()
}
