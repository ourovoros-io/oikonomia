/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, test } from 'vitest'

import { I18nProvider, useI18n } from './I18nProvider'
import { LOCALE_STORAGE_KEY, getLocale, resetI18nForTests, setLocale } from './i18n'

function Probe() {
  const { locale, t } = useI18n()
  return (
    <div>
      <span>{locale}</span>
      <h1>{t('settings.title')}</h1>
    </div>
  )
}

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

describe('I18nProvider', () => {
  test('hydrates locale from oikonomia.locale', async () => {
    localStorage.setItem(LOCALE_STORAGE_KEY, 'el')
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(getLocale()).toBe('el')
      expect(screen.getByText('el')).toBeTruthy()
    })
    expect(screen.getByRole('heading', { name: 'Ρυθμίσεις' })).toBeTruthy()
  })

  test('missing key defaults to en', async () => {
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    await waitFor(() => {
      expect(getLocale()).toBe('en')
    })
    expect(screen.getByRole('heading', { name: 'Settings' })).toBeTruthy()
  })

  test('setLocale writes oikonomia.locale only', async () => {
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    )
    setLocale('el')
    await waitFor(() => {
      expect(getLocale()).toBe('el')
      expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe('el')
      expect(screen.getByText('el')).toBeTruthy()
    })
  })
})
