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
  database: 'error.io',
  io: 'error.io',
  serialization: 'error.io',
  crypto: 'error.crypto',
  vault_corrupt: 'error.vaultCorrupt',
  // Borrowed copy: a vault from a newer build is sound and needs wording of its own.
  vault_too_new: 'error.vaultCorrupt',
  backup_invalid: 'error.backupInvalid',
  restore_would_overwrite: 'error.restoreWouldOverwrite',
  not_found: 'error.notFound',
  analysis: 'error.analysis',
  // Every CSV code borrows one sentence until each has wording of its own; an
  // amount that does not fit reads as every other amount that does not.
  csv_empty: 'error.csvParse',
  csv_not_utf8: 'error.csvParse',
  csv_too_large: 'error.csvParse',
  csv_parse: 'error.csvParse',
  csv_missing_header: 'error.csvParse',
  csv_missing_date_column: 'error.csvParse',
  csv_missing_amount_column: 'error.csvParse',
  csv_missing_column: 'error.csvParse',
  csv_invalid_date: 'error.csvParse',
  csv_invalid_amount: 'error.csvParse',
  csv_invalid_type: 'error.csvParse',
  csv_invalid_status: 'error.csvParse',
  csv_invalid_integer: 'error.csvParse',
  csv_missing_date: 'error.csvParse',
  csv_missing_amount: 'error.csvParse',
  csv_amount_overflow: 'error.moneyOverflow',
  csv_zero_amount: 'error.csvParse',
  csv_invalid_mapping: 'error.csvParse',
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
  update_response_too_large: 'error.update',
  update_manifest_signature: 'error.update',
  update_manifest_parse: 'error.update',
  update_invalid_version: 'error.update',
  update_missing_platform: 'error.update',
  update_artifact_url: 'error.update',
  update_artifact_integrity: 'error.update',
  update_cache_io: 'error.update',
  update_invalid_feed_url: 'error.update',
  update_invalid_feed_input: 'error.update',
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

/** A copy key for `code` in `map`, ignoring names inherited from Object.prototype. */
function keyFor(map: Record<string, string>, code: string): string | undefined {
  return Object.hasOwn(map, code) ? map[code] : undefined
}

/**
 * Codes whose copy says little ("A file operation failed."). When a screen
 * passes its own sentence for the failure, that sentence says more, so it wins
 * over these. Every other code is specific enough to show as it is.
 */
const VAGUE_CODES: ReadonlySet<string> = new Set([
  'database',
  'io',
  'serialization',
  'crypto',
  'unknown',
  'task_failed',
  'validation_internal',
  'analysis',
])

/**
 * Codes whose copy names an account role with `{role}`, mapped to the copy
 * used when the role is missing or has no label. Rust sends the role as an
 * identifier (see accountRoles.json); the label is the one the entry form shows.
 */
const ROLE_FALLBACK_KEYS: Record<string, string> = {
  'error.accountRequired': 'error.accountRequiredGeneric',
  'error.accountWrongType': 'error.accountWrongTypeGeneric',
}

/** Every `{name}` the copy under `key` fills in. */
function placeholdersOf(key: string): string[] {
  return [...t(key).matchAll(/\{(\w+)\}/g)].map((match) => match[1])
}

/** True when the copy under `key` names a value that `params` does not carry. */
function lacksParams(key: string, params: Record<string, string> | undefined): boolean {
  return placeholdersOf(key).some((name) => params?.[name] === undefined)
}

/**
 * Copy that names the account role, or the sentence without one when the role
 * has no label. Undefined when the copy that would show still asks for a value
 * `params` does not carry, so no raw `{name}` is ever shown.
 */
function roleCopy(
  key: string,
  fallbackKey: string,
  params: Record<string, string> | undefined,
): string | undefined {
  const labelKey = `error.role.${params?.role ?? ''}`
  const label = t(labelKey)

  if (params?.role && label !== labelKey) {
    const labelled = { ...params, role: label }
    const copy = t(key, labelled)

    if (copy !== key) return lacksParams(key, labelled) ? undefined : copy
  }

  const copy = t(fallbackKey, params)

  return lacksParams(fallbackKey, params) ? undefined : copy
}

/**
 * Log the raw error for diagnosis. The sentence the user sees is localized and
 * often vague, so the operating-system detail in `message` is kept here, once,
 * and never shown.
 */
export function logCommandError(err: unknown): void {
  const cmd = asCommandError(err)
  const params = cmd.params ? ` ${JSON.stringify(cmd.params)}` : ''

  console.warn(`Command error "${cmd.code}"${params}: ${cmd.message}`)
}

/**
 * The one way to show a failed command to the user: localized copy for the
 * error's `code`, with its `params` filled in, never the raw Rust text (which
 * is English and can carry operating-system detail).
 *
 * `fallbackKey` is the screen's own sentence for the failure. It is used when
 * the code has no copy, and also in place of the copy of a vague code (see
 * VAGUE_CODES), because "Could not delete the document." says more than
 * "A file operation failed.". A specific code always shows its own copy. With
 * no `fallbackKey` the code's copy, or the generic one, is shown.
 *
 * Copy that needs a value the error did not carry is treated as no copy, so a
 * raw `{name}` is never shown.
 *
 * Vague, unknown and copy-less errors are logged, so their detail is not lost.
 * User-correctable errors (a name already taken) are not diagnostic and are not.
 */
export function commandErrorMessage(err: unknown, fallbackKey?: string): string {
  const cmd = asCommandError(err)
  const vague = VAGUE_CODES.has(cmd.code)
  const key = keyFor(ERROR_CODE_KEYS, cmd.code) ?? keyFor(WEB_ERROR_KEYS, cmd.code)

  let copy: string | undefined
  if (key) {
    const roleFallbackKey = keyFor(ROLE_FALLBACK_KEYS, key)
    // Copy that still asks for a value the error did not carry would show a
    // raw `{name}`, so it counts as no copy.
    if (roleFallbackKey) {
      copy = roleCopy(key, roleFallbackKey, cmd.params)
    } else if (!lacksParams(key, cmd.params)) {
      const text = t(key, cmd.params)

      if (text !== key) copy = text
    }
  }

  if (vague || copy === undefined) logCommandError(cmd)

  if (vague && fallbackKey !== undefined) return t(fallbackKey)

  return copy ?? t(fallbackKey ?? 'error.unknown')
}

/** Tauri reports an unregistered command; the FE stub may answer instead. */
export function isMissingIpcCommand(err: { code: string; message: string }, command: string): boolean {
  const haystack = `${err.code} ${err.message}`.toLowerCase()
  const name = command.toLowerCase()

  return haystack.includes(name) && (haystack.includes('not found') || haystack.includes('unknown'))
}
