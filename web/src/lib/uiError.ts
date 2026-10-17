import { asCommandError, commandErrorText, logHiddenCause } from './commandError'
import { t, type MessageKey, type TranslateVars } from './i18n'
import type { Currency } from './money'
import type { CommandError } from './tauri'
import { quietUiTexts, renderUiTexts, type UiText } from './uiText'

/**
 * An error a screen shows, kept as data and worded when it is drawn
 * (`uiErrorText`), so switching language while it shows re-words it. State
 * holds one of these, never a finished sentence.
 *
 * - `copy`: a catalog sentence and the values it fills in.
 * - `command`: a failed command, worded by its code (`commandErrorText`).
 * - `notes`: what a document analysis said about the file, worded for the book
 *   it was read for; `fallbackKey` when none of it can be worded.
 *
 * Builders that take a caught command error are named `*Failure`
 * (`commandFailure`, `backupFailure`); the others `*Error`.
 */
export type UiError =
  | { kind: 'copy'; key: MessageKey; vars?: TranslateVars }
  | { kind: 'command'; error: CommandError; fallbackKey?: MessageKey }
  | { kind: 'notes'; notes: readonly UiText[]; book: Currency | null; fallbackKey: MessageKey }

/** A catalog sentence as an error. */
export function copyError(key: MessageKey, vars?: TranslateVars): UiError {
  return vars ? { kind: 'copy', key, vars } : { kind: 'copy', key }
}

/**
 * A failed command as an error, from whatever `invoke` threw. Its cause is
 * logged here, once, when the sentence will hide it; wording it later never logs.
 */
export function commandFailure(err: unknown, fallbackKey?: MessageKey): UiError {
  const error = asCommandError(err)
  logHiddenCause(error)

  return fallbackKey === undefined
    ? { kind: 'command', error }
    : { kind: 'command', error, fallbackKey }
}

/**
 * Analysis notes as an error, worded for `book`, or `fallbackKey` when none can
 * be. A note this version cannot word is logged here, once; wording it later
 * never logs.
 */
export function notesError(
  notes: readonly UiText[],
  book: Currency | null,
  fallbackKey: MessageKey,
): UiError {
  renderUiTexts(notes, book)

  return { kind: 'notes', notes, book, fallbackKey }
}

/** The sentence for `error` in the current language. It never logs, so it may run on every draw. */
export function uiErrorText(error: UiError): string {
  switch (error.kind) {
    case 'copy':
      return t(error.key, error.vars)
    case 'command':
      return commandErrorText(error.error, error.fallbackKey)
    case 'notes':
      return quietUiTexts(error.notes, error.book) || t(error.fallbackKey)
  }
}
