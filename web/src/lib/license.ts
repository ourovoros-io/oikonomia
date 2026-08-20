import { t, type Locale } from './i18n'
import type { CommandError } from './tauri'

export type LicenseState = 'none' | 'trial' | 'licensed' | 'expired'

/** Snake_case matches the Rust `license_status` / `license_install` payload. */
export type LicenseStatus = {
  state: LicenseState
  days_remaining?: number
  /** ISO `YYYY-MM-DD` when `state` is licensed (or a lapsed license). */
  licensed_until?: string
}

export function isLicenseExpiredCode(code: string): boolean {
  return code === 'license_expired'
}

/**
 * Import failures map `CommandError.code` only. Native today sends
 * `license_invalid`; we cannot tell signature / wrong product / unreadable
 * apart, so those collapse to the generic Writer string. Never use Rust Display.
 */
export function licenseImportError(err: CommandError): string {
  switch (err.code) {
    case 'license_signature':
    case 'signature':
      return t('settings.license.error.signature')
    case 'license_wrong_product':
    case 'wrong_product':
      return t('settings.license.error.wrongProduct')
    case 'license_unreadable':
    case 'unreadable':
      return t('settings.license.error.unreadable')
    default:
      return t('settings.license.error.generic')
  }
}

/** Prefer the license-ended copy when a calendar date is known; else trial. */
export function licenseExpiredBanner(status: Pick<LicenseStatus, 'licensed_until'>): string {
  return status.licensed_until
    ? t('settings.license.banner.expired')
    : t('settings.trial.banner.expired')
}

/** Local short date for `settings.license.licensedUntil` (en-GB → `20 Aug 2027`). */
export function formatLicensedUntil(iso: string, locale: Locale): string {
  const parsed = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso)
  if (!parsed) return iso
  const date = new Date(Number(parsed[1]), Number(parsed[2]) - 1, Number(parsed[3]))
  if (Number.isNaN(date.getTime())) return iso
  try {
    return new Intl.DateTimeFormat(locale === 'el' ? 'el-GR' : 'en-GB', {
      day: 'numeric',
      month: 'short',
      year: 'numeric',
    }).format(date)
  } catch {
    return iso
  }
}
