import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
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
}

const I18nContext = createContext<I18nContextValue | null>(null)

/**
 * Hydrates locale from UiPrefs (`settings_get_locale` / `settings_get_ui_prefs`).
 * localStorage is first-paint cache only. The Settings language pill calls setLocale.
 */
export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() => readCachedLocale() ?? getLocale())

  useEffect(() => {
    const cached = readCachedLocale()
    if (cached) applyLocale(cached)
    return subscribeLocale(() => setLocaleState(getLocale()))
  }, [])

  useEffect(() => {
    setLocalePersist((next) => api.setLocale(next).catch(() => undefined))
    return () => setLocalePersist(null)
  }, [])

  useEffect(() => {
    let cancelled = false

    function applyFromPrefsOrCache(prefsLocale: unknown): void {
      if (cancelled) return
      if (prefsLocale === 'en' || prefsLocale === 'el') {
        applyLocale(prefsLocale)
        return
      }
      applyLocale(readCachedLocale() ?? 'en')
    }

    void api
      .getLocale()
      .then((next) => {
        if (!cancelled) applyLocale(parseLocale(next))
      })
      .catch(() => {
        if (cancelled) return
        void api
          .getUiPrefs()
          .then((prefs) => applyFromPrefsOrCache(prefs.locale))
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
    }),
    [locale],
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
  return { locale: getLocale(), t: translate, setLocale: setLocaleAndPersist }
}
