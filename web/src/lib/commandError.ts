import { withShortFileText } from './fileText'
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
  vault_unlock_before_backup: 'error.vaultUnlockBeforeBackup',
  vault_too_new: 'error.vaultTooNew',
  backup_invalid: 'error.backupInvalid',
  restore_would_overwrite: 'error.restoreWouldOverwrite',
  not_found: 'error.notFound',
  analysis: 'error.analysis',
  csv_empty: 'error.csvEmpty',
  csv_not_utf8: 'error.csvNotUtf8',
  csv_too_large: 'error.csvTooLarge',
  csv_parse: 'error.csvParse',
  csv_missing_header: 'error.csvMissingHeader',
  csv_missing_column: 'error.csvMissingColumn',
  // A problem with one cell is worded by the sentence the import preview
  // shows for a row with the same problem (NOTE_CODE_KEYS in uiText.ts), so
  // one problem reads one way wherever it turns up.
  csv_invalid_date: 'tx.csv.rowProblem.invalidDate',
  csv_invalid_amount: 'tx.csv.rowProblem.invalidAmount',
  csv_invalid_type: 'tx.csv.rowProblem.invalidType',
  csv_invalid_status: 'error.csvInvalidStatus',
  csv_invalid_integer: 'error.csvInvalidInteger',
  csv_missing_date: 'tx.csv.rowProblem.missingDate',
  csv_missing_amount: 'tx.csv.rowProblem.missingAmount',
  csv_amount_overflow: 'tx.csv.rowProblem.amountOverflow',
  csv_zero_amount: 'tx.csv.rowProblem.zeroAmount',
  // The sentence for a mapping problem this version has no wording for; each
  // known problem has its own in VARIANT_KEYS.
  csv_invalid_mapping: 'error.csvInvalidMapping',
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
  // The sentence names no dialog, so it reads the same for a file to open.
  open_location_invalid: 'error.saveLocationInvalid',
  save_failed: 'error.saveFailed',
  path_not_granted: 'error.pathNotGranted',
  mail_client_failed: 'error.mailClientFailed',
  task_failed: 'error.taskFailed',
  // Only the update commands need the cache directory.
  cache_dir_unavailable: 'error.update',
  // Sent while a failed start is being reported natively; no screen is up to
  // show it, so it has no sentence of its own.
  app_state_unavailable: 'error.unknown',
  update_install_not_allowed: 'error.update',
  update_missing_public_key: 'error.update',
  update_network: 'error.update',
  update_manifest_signature: 'error.update',
  update_manifest_parse: 'error.update',
  update_invalid_version: 'error.update',
  update_missing_platform: 'error.update',
  update_artifact_url: 'error.update',
  update_artifact_integrity: 'error.update',
  update_artifact_too_large: 'error.update',
  update_cache_io: 'error.update',
  update_install_failed: 'error.update',
  update_invalid_feed_url: 'error.update',
  update_invalid_feed_input: 'error.update',
  unknown: 'error.unknown',
}

/**
 * Codes whose sentence depends on an identifier Rust sends in one parameter:
 * the parameter that holds it, and the i18n key for each identifier. The
 * identifier chooses the sentence and is never shown. One this version does
 * not list falls back to the code's own key in ERROR_CODE_KEYS.
 *
 * The identifiers of `csv_invalid_mapping` are pinned by
 * csvMappingProblems.json on both sides (`CsvMappingProblem::identifier`).
 */
export const VARIANT_KEYS: Record<string, { param: string; keys: Record<string, string> }> = {
  csv_invalid_mapping: {
    param: 'problem',
    keys: {
      missing_date: 'error.csvMapping.missingDate',
      missing_amount: 'error.csvMapping.missingAmount',
      amount_and_debit_or_credit: 'error.csvMapping.amountAndDebitOrCredit',
      unknown_column: 'error.csvMapping.unknownColumn',
    },
  },
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
  'app_state_unavailable',
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
 * The i18n key for the identifier `code` carries in its variant parameter,
 * or undefined when the code has no variants or the identifier is not listed.
 */
function variantKey(code: string, params: Record<string, string> | undefined): string | undefined {
  if (!Object.hasOwn(VARIANT_KEYS, code)) return undefined

  const { param, keys } = VARIANT_KEYS[code]

  return keyFor(keys, params?.[param] ?? '')
}

/**
 * The copy under `key` with `params` filled in. Undefined when the catalog
 * has no such key or the copy asks for a value `params` does not carry, so a
 * raw key or a raw `{name}` is never shown.
 */
function filledCopy(key: string, params: Record<string, string> | undefined): string | undefined {
  if (lacksParams(key, params)) return undefined

  const text = t(key, params)

  return text === key ? undefined : text
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

  // Text from the user's file is cut to a length a sentence can hold.
  const params = cmd.params ? withShortFileText(cmd.params) : undefined

  let copy: string | undefined
  if (key) {
    const roleFallbackKey = keyFor(ROLE_FALLBACK_KEYS, key)
    const variant = variantKey(cmd.code, params)

    if (roleFallbackKey) {
      copy = roleCopy(key, roleFallbackKey, params)
    } else {
      // The sentence for the identifier Rust sent, when there is one and it
      // has every value it names; otherwise the code's own sentence.
      copy = (variant && filledCopy(variant, params)) ?? filledCopy(key, params)
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
