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

/**
 * The single place that turns whatever `invoke` threw into a CommandError.
 * It keeps `code` and `params` so the UI can localize by code; `message` is
 * only for logs and is never shown.
 */
export function asCommandError(err: unknown): CommandError {
  if (typeof err === 'string') {
    return { code: 'unknown', message: err }
  }

  if (err && typeof err === 'object') {
    const obj = err as Record<string, unknown>

    // Tauri often wraps the payload as { message, code } or { error, ... }.
    if (typeof obj.message === 'string') {
      return {
        code: typeof obj.code === 'string' ? obj.code : 'unknown',
        message: obj.message,
        ...(obj.params && typeof obj.params === 'object'
          ? { params: obj.params as Record<string, string> }
          : {}),
      }
    }

    if (typeof obj.error === 'string') {
      return { code: 'unknown', message: obj.error }
    }
  }

  if (err instanceof Error) {
    return { code: 'unknown', message: err.message }
  }

  return { code: 'unknown', message: String(err) }
}

/**
 * Codes raised by the web layer itself, for failures that never reach Rust
 * (a browser FileReader error, for example). They share the display path of
 * the Rust codes, but they are not in errorCodes.json.
 */
export const WEB_ERROR_KEYS: Record<string, string> = {
  file_read_failed: 'files.error.read',
}

/** The error a failed browser file read raises, so it is localized like any other. */
export function fileReadError(): CommandError {
  return { code: 'file_read_failed', message: 'Could not read file' }
}

/**
 * The one way to show a failed command to the user: localized copy for the
 * error's `code`, with its `params` filled in. A code with no copy gets the
 * screen's own localized fallback (`fallbackKey`), never the raw Rust text,
 * which is English and can carry operating-system detail. The raw message
 * goes to the console for diagnosis only.
 */
export function commandErrorMessage(err: unknown, fallbackKey = 'error.unknown'): string {
  const cmd = asCommandError(err)

  // 'unknown' is Rust's catch-all: the screen's own sentence says more than a generic one.
  const key = cmd.code === 'unknown' ? undefined : (ERROR_CODE_KEYS[cmd.code] ?? WEB_ERROR_KEYS[cmd.code])
  if (key) {
    const copy = t(key, cmd.params)
    if (copy !== key) return copy
  }

  console.warn(`Unlocalized command error "${cmd.code}": ${cmd.message}`)

  return t(fallbackKey)
}
