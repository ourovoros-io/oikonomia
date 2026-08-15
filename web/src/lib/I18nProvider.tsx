import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import {
  getLocale,
  hydrateLocaleFromStorage,
  setLocale as setLocaleAndPersist,
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
 * Hydrates locale from `localStorage['oikonomia.locale']` (`en` | `el`).
 * No UiPrefs / settings_*_locale dual-write yet. Unlock and Quick Add inherit.
 */
export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() => hydrateLocaleFromStorage())

  useEffect(() => subscribeLocale(() => setLocaleState(getLocale())), [])

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
