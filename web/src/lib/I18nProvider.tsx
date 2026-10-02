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
  applyLocaleFromPrefs,
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
 * Hydrates locale from UiPrefs (`settings_get_locale` / `settings_get_ui_prefs`).
 * localStorage is an optimistic mirror only. The Settings language pill calls setLocale.
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
      }
    })

    return () => setLocalePersist(null)
  }, [])

  useEffect(() => {
    let cancelled = false

    void api
      .getLocale()
      .then((next) => {
        if (cancelled) return

        storedLocale.current = parseLocale(next)
        applyLocale(storedLocale.current)
      })
      .catch(() => {
        if (cancelled) return
        void api
          .getUiPrefs()
          .then((prefs) => {
            if (!cancelled) storedLocale.current = applyLocaleFromPrefs(prefs)
          })
          .catch(() => {
            if (!cancelled) applyLocale(readCachedLocale() ?? 'en')
          })
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
