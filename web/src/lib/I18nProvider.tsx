import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { api } from './api'
import {
  applyLocale,
  getLocale,
  parseLocale,
  readCachedLocale,
  setLocale as setLocaleAndPersist,
  setLocalePersist,
  subscribeLocale,
  t as translate,
  type Locale,
  type TranslateVars,
} from './i18n'

type I18nContextValue = {
  locale: Locale
  t: (key: string, vars?: TranslateVars) => string
  setLocale: (locale: Locale) => void
  /**
   * True when the last language change could not be saved, so the interface
   * went back to the stored language. Cleared by the next change that saves.
   */
  languageChangeFailed: boolean
}

const I18nContext = createContext<I18nContextValue | null>(null)

/**
 * The language the backend has stored, read again after a change failed to
 * save. When it cannot be read either, the last language known to be stored.
 */
async function readStoredLocale(lastKnown: Locale): Promise<Locale> {
  try {
    return parseLocale(await api.getLocale())
  } catch {
    return lastKnown
  }
}

/**
 * The system's preferred languages, most preferred first. Only reported to
 * Rust, which decides whether and how to use them.
 */
function systemLanguages(): string[] {
  const languages = navigator.languages ?? []
  if (languages.length > 0) return [...languages]

  return navigator.language ? [navigator.language] : []
}

type Hydrated = {
  /** What the backend has stored; null when it could not be read. */
  stored: Locale | null
  /** What the interface shows. */
  applied: Locale
}

/**
 * Ask Rust for the language: `settings_resolve_locale` picks it from the
 * system on a first run and otherwise returns the stored one. Falls back to
 * the plain getter, then the prefs, then the optimistic cache, so a failing
 * command (or no Tauri at all: browser preview, tests) never blocks the UI.
 */
async function hydrateLocale(): Promise<Hydrated> {
  try {
    const stored = parseLocale(await api.resolveLocale(systemLanguages()))
    return { stored, applied: stored }
  } catch {
    // fall through to the older reads
  }

  try {
    const stored = parseLocale(await api.getLocale())
    return { stored, applied: stored }
  } catch {
    // fall through
  }

  try {
    const prefs = await api.getUiPrefs()
    const stored = parseLocale(prefs?.locale)
    return { stored, applied: stored }
  } catch {
    return { stored: null, applied: readCachedLocale() ?? 'en' }
  }
}

/**
 * Hydrates the locale from Rust: `settings_resolve_locale` (which stores the
 * system language on a first run), falling back to `settings_get_locale`, then
 * `settings_get_ui_prefs`. localStorage is an optimistic mirror only. The Settings language pill calls setLocale.
 */
export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() => {
    const cached = readCachedLocale()
    if (cached) applyLocale(cached)
    return getLocale()
  })

  useEffect(() => subscribeLocale(() => setLocaleState(getLocale())), [])

  // CSS uppercase (small-caps labels, eyebrows) keeps a locale's own
  // combining marks only when lang matches the text's language; a stale
  // static lang="en" uppercases Greek as if it were English and drops the
  // tonos (e.g. ΗΜΕΡΟΛΌΓΙΟ loses its accent).
  useEffect(() => {
    document.documentElement.lang = locale
  }, [locale])

  const [languageChangeFailed, setLanguageChangeFailed] = useState(false)

  // The language the backend is known to have stored. Stored text (seeded
  // names, generated descriptions) follows the stored preference, so the
  // interface must never stay in a language that failed to save.
  const storedLocale = useRef<Locale>(locale)
  const latestChange = useRef(0)
  // The last change of this window whose save has answered; a change is in
  // flight while this is behind latestChange.
  const settledChange = useRef(0)

  useEffect(() => {
    setLocalePersist(async (next) => {
      const change = ++latestChange.current

      try {
        await api.setLocale(next)
        storedLocale.current = next

        if (change === latestChange.current) setLanguageChangeFailed(false)
      } catch {
        // A newer change owns the outcome; reverting here would undo it.
        if (change !== latestChange.current) return

        const stored = await readStoredLocale(storedLocale.current)

        // The re-read took time; a change made meanwhile owns the outcome.
        if (change !== latestChange.current) return

        storedLocale.current = stored
        applyLocale(stored)
        setLanguageChangeFailed(true)
      } finally {
        if (change === latestChange.current) settledChange.current = change
      }
    })

    return () => setLocalePersist(null)
  }, [])

  // Rust sends every window the language it stores, so a window that stays
  // open while another changes it (the hidden quick-add window, while
  // Settings changes it) follows. A change of this window's own still in
  // flight owns the outcome, so an older language arriving meanwhile does
  // not flash up; Rust sends the events in the order it put the languages
  // into effect, so the last one this window gets is the language in effect.
  const eventsReceived = useRef(0)
  useEffect(
    () =>
      api.onLocaleChanged((sent) => {
        eventsReceived.current += 1
        const stored = parseLocale(sent)
        storedLocale.current = stored

        if (settledChange.current === latestChange.current) applyLocale(stored)
      }),
    [],
  )

  useEffect(() => {
    let cancelled = false

    // A change the user makes, or one another window makes and Rust sends,
    // while this is in flight owns the outcome: it is newer than what the
    // hydration read, so the late result must not repaint the interface.
    const startedAt = latestChange.current
    const eventsBefore = eventsReceived.current
    const superseded = () =>
      cancelled ||
      latestChange.current !== startedAt ||
      eventsReceived.current !== eventsBefore

    void hydrateLocale().then((hydrated) => {
      if (superseded()) return

      if (hydrated.stored) storedLocale.current = hydrated.stored
      applyLocale(hydrated.applied)
    })

    return () => {
      cancelled = true
    }
  }, [])

  const value = useMemo<I18nContextValue>(
    () => ({
      locale,
      t: translate,
      setLocale: setLocaleAndPersist,
      languageChangeFailed,
    }),
    [locale, languageChangeFailed],
  )

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
}

// This rule only keeps fast refresh in development: with the hook beside the
// provider, editing this file reloads the page instead. Splitting the file
// for that is not worth moving the provider every page imports from.
// eslint-disable-next-line react/only-export-components
export function useI18n(): I18nContextValue {
  const ctx = useContext(I18nContext)
  const [, bump] = useState(0)

  useEffect(() => {
    if (ctx) return
    return subscribeLocale(() => bump((n) => n + 1))
  }, [ctx])

  if (ctx) return ctx
  return {
    locale: getLocale(),
    t: translate,
    setLocale: setLocaleAndPersist,
    languageChangeFailed: false,
  }
}
