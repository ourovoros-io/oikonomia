/** @vitest-environment jsdom */

import { afterEach, describe, expect, test } from 'vitest'
import {
  canAddAnotherBook,
  formatLicensedUntil,
  isLicenseExpiredCode,
  licenseErrorMessage,
  licenseExpiredBanner,
  licenseImportError,
} from './license'
import { resetI18nForTests, setLocale } from './i18n'

afterEach(() => {
  resetI18nForTests()
})

describe('licenseImportError', () => {
  test('license_invalid uses generic Writer copy, not Rust Display', () => {
    expect(
      licenseImportError({
        code: 'license_invalid',
        message: 'ed25519: signature verification failed on blob',
      }),
    ).toBe('Could not import the license.')
  })

  test('unknown import code still uses generic Writer copy', () => {
    expect(licenseImportError({ code: 'unknown', message: 'LicenseError Display' })).toBe(
      'Could not import the license.',
    )
  })
})

describe('licenseErrorMessage', () => {
  test('license_entity_limit uses Writer copy, not Rust Display', () => {
    expect(licenseErrorMessage('license_entity_limit')).toBe(
      'A license is required to add another book.',
    )
  })

  test('license_expired is a state change, not a mapped string', () => {
    expect(licenseErrorMessage('license_expired')).toBeUndefined()
  })
})

describe('canAddAnotherBook', () => {
  test('first book is allowed without a license', () => {
    expect(canAddAnotherBook(null, 0)).toBe(true)
    expect(canAddAnotherBook({ state: 'trial', days_remaining: 12 }, 0)).toBe(true)
    expect(canAddAnotherBook({ state: 'expired' }, 0)).toBe(true)
  })

  test('a second book requires licensed', () => {
    expect(canAddAnotherBook({ state: 'trial', days_remaining: 12 }, 1)).toBe(false)
    expect(canAddAnotherBook({ state: 'none' }, 1)).toBe(false)
    expect(canAddAnotherBook({ state: 'expired' }, 1)).toBe(false)
    expect(canAddAnotherBook(null, 1)).toBe(false)
    expect(canAddAnotherBook({ state: 'licensed', licensed_until: '2027-08-20' }, 1)).toBe(
      true,
    )
  })
})

describe('isLicenseExpiredCode', () => {
  test('only license_expired is the expired state', () => {
    expect(isLicenseExpiredCode('license_expired')).toBe(true)
    expect(isLicenseExpiredCode('license_invalid')).toBe(false)
  })
})

describe('licenseExpiredBanner', () => {
  test('trial copy when no licensed_until', () => {
    expect(licenseExpiredBanner({})).toBe(
      'Trial ended. You can still back up, restore, and export CSV.',
    )
  })

  test('license copy when a licensed_until date is known', () => {
    expect(licenseExpiredBanner({ licensed_until: '2027-08-20' })).toBe(
      'License ended. Backup, restore, and CSV export still work.',
    )
  })
})

describe('formatLicensedUntil', () => {
  test('formats ISO date locally for English (en-GB short month)', () => {
    expect(formatLicensedUntil('2027-08-20', 'en').replace(/\u00a0|\u202f/g, ' ')).toBe(
      '20 Aug 2027',
    )
  })

  test('returns the raw value when the date is not ISO', () => {
    expect(formatLicensedUntil('not-a-date', 'en')).toBe('not-a-date')
  })

  test('el catalog interpolation still receives a formatted date', () => {
    setLocale('el')
    const date = formatLicensedUntil('2027-08-20', 'el')
    expect(date.length).toBeGreaterThan(0)
    expect(date).not.toBe('2027-08-20')
  })
})
