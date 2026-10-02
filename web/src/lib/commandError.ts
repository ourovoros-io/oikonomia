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
  io: 'error.io',
  crypto: 'error.crypto',
  vault_corrupt: 'error.vaultCorrupt',
  backup_invalid: 'error.backupInvalid',
  restore_would_overwrite: 'error.restoreWouldOverwrite',
  not_found: 'error.notFound',
  analysis: 'error.analysis',
  csv_parse: 'error.csvParse',
  password_too_short: 'error.passwordTooShort',
  name_required: 'error.nameRequired',
  name_taken: 'error.nameTaken',
  account_code_taken: 'error.accountCodeTaken',
  system_account_protected: 'error.systemAccountProtected',
  account_inactive: 'error.accountInactive',
  account_required: 'error.accountRequired',
  account_wrong_type: 'error.accountWrongType',
  same_account: 'error.sameAccount',
  amount_not_positive: 'error.amountNotPositive',
  bill_status_required: 'error.billStatusRequired',
  invalid_date: 'error.invalidDate',
  date_range_inverted: 'error.dateRangeInverted',
  date_out_of_range: 'error.dateOutOfRange',
  day_of_month_invalid: 'error.dayOfMonthInvalid',
  lock_timeout_too_short: 'error.lockTimeoutTooShort',
  currency_invalid: 'error.currencyInvalid',
  entry_already_voided: 'error.entryAlreadyVoided',
  entry_not_posted: 'error.entryNotPosted',
  wrong_book: 'error.wrongBook',
  opening_balance_account_type: 'error.openingBalanceAccountType',
  opening_balance_unchanged: 'error.openingBalanceUnchanged',
  no_equity_account: 'error.noEquityAccount',
  file_empty: 'error.fileEmpty',
  file_too_large: 'error.fileTooLarge',
  file_type_unsupported: 'error.fileTypeUnsupported',
  vault_already_initialized: 'error.vaultAlreadyInitialized',
  validation_internal: 'error.validationInternal',
  file_data_invalid: 'error.fileDataInvalid',
  file_unreadable: 'error.fileUnreadable',
  save_location_invalid: 'error.saveLocationInvalid',
  save_failed: 'error.saveFailed',
  path_not_granted: 'error.pathNotGranted',
  mail_client_failed: 'error.mailClientFailed',
  task_failed: 'error.taskFailed',
  update_install_not_allowed: 'error.update',
  update_missing_public_key: 'error.update',
  update_network: 'error.update',
  update_manifest_signature: 'error.update',
  update_manifest_parse: 'error.update',
  update_artifact_url: 'error.update',
  update_artifact_integrity: 'error.update',
  update_invalid_feed_url: 'error.update',
  unknown: 'error.unknown',
}

/** Localized copy for a known command error code, filled in from its params; raw English otherwise. */
export function commandErrorMessage(err: CommandError): string {
  const key = ERROR_CODE_KEYS[err.code]
  if (key) {
    const copy = t(key, err.params)
    if (copy !== key) return copy
  }
  return err.message
}
