import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
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
}

const I18nContext = createContext<I18nContextValue | null>(null)

/**
 * Hydrates locale from UiPrefs (`settings_get_locale` / `settings_get_ui_prefs`).
 * localStorage is an optimistic mirror only. Settings language chrome is held.
 */
export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() => {
    const cached = readCachedLocale()
    if (cached) applyLocale(cached)
    return getLocale()
  })

  useEffect(() => subscribeLocale(() => setLocaleState(getLocale())), [])

  useEffect(() => {
    setLocalePersist((next) => api.setLocale(next).catch(() => undefined))
    return () => setLocalePersist(null)
  }, [])

  useEffect(() => {
    let cancelled = false

    void api
      .getLocale()
      .then((next) => {
        if (!cancelled) applyLocale(parseLocale(next))
      })
      .catch(() => {
        if (cancelled) return
        void api
          .getUiPrefs()
          .then((prefs) => {
            if (!cancelled) applyLocaleFromPrefs(prefs)
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
