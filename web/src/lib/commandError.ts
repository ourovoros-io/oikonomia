import { t } from './i18n'
import type { CommandError } from './tauri'

/** Canonical code -> i18n key. Pinned by errorCodes.json on both sides. */
export const ERROR_CODE_KEYS: Record<string, string> = {
  vault_uninitialized: 'error.vaultUninitialized',
  vault_locked: 'error.vaultLocked',
  invalid_password: 'error.invalidPassword',
  unbalanced_entry: 'error.unbalancedEntry',
  too_few_lines: 'error.tooFewLines',
  invalid_line_amounts: 'error.invalidLineAmounts',
  account_wrong_entity: 'error.accountWrongEntity',
  money_overflow: 'error.moneyOverflow',
  negative_money: 'error.negativeMoney',
  validation: 'error.validation',
  io: 'error.io',
  crypto: 'error.crypto',
  vault_corrupt: 'error.vaultCorrupt',
  backup_invalid: 'error.backupInvalid',
  restore_would_overwrite: 'error.restoreWouldOverwrite',
  not_found: 'error.notFound',
  analysis: 'error.analysis',
  csv_parse: 'error.csvParse',
  license_invalid: 'error.licenseInvalid',
  license_expired: 'error.licenseExpired',
  license_entity_limit: 'error.licenseEntityLimit',
  unknown: 'error.unknown',
}

/** Localized copy for a known command error code; raw English otherwise. */
export function commandErrorMessage(err: CommandError): string {
  const key = ERROR_CODE_KEYS[err.code]
  if (key) {
    const copy = t(key)
    if (copy !== key) return copy
  }
  return err.message
}
