import { asCommandError, commandErrorText, logHiddenCause } from './commandError'
import { t, type TranslateVars } from './i18n'
import type { Currency } from './money'
import type { CommandError } from './tauri'
import { renderUiTexts, type UiText } from './uiText'

/**
 * An error a screen shows, kept as data and worded when it is drawn
 * (`uiErrorText`), so switching language while it shows re-words it. State
 * holds one of these, never a finished sentence.
 *
 * - `copy`: a catalog sentence and the values it fills in.
 * - `command`: a failed command, worded by its code (`commandErrorText`).
 * - `notes`: what a document analysis said about the file, worded for the book
 *   it was read for; `fallbackKey` when none of it can be worded.
 */
export type UiError =
  | { kind: 'copy'; key: string; vars?: TranslateVars }
  | { kind: 'command'; error: CommandError; fallbackKey?: string }
  | { kind: 'notes'; notes: readonly UiText[]; book: Currency | null; fallbackKey: string }

/** A catalog sentence as an error. */
export function copyError(key: string, vars?: TranslateVars): UiError {
  return vars ? { kind: 'copy', key, vars } : { kind: 'copy', key }
}

/**
 * A failed command as an error, from whatever `invoke` threw. Its cause is
 * logged here, once, when the sentence will hide it; wording it later never logs.
 */
export function commandFailure(err: unknown, fallbackKey?: string): UiError {
  const error = asCommandError(err)
  logHiddenCause(error)

  return fallbackKey === undefined
    ? { kind: 'command', error }
    : { kind: 'command', error, fallbackKey }
}

/** Analysis notes as an error, worded for `book`, or `fallbackKey` when none can be. */
export function notesError(
  notes: readonly UiText[],
  book: Currency | null,
  fallbackKey: string,
): UiError {
  return { kind: 'notes', notes, book, fallbackKey }
}

/** The sentence for `error` in the current language. */
export function uiErrorText(error: UiError): string {
  switch (error.kind) {
    case 'copy':
      return t(error.key, error.vars)
    case 'command':
      return commandErrorText(error.error, error.fallbackKey)
    case 'notes':
      return renderUiTexts(error.notes, error.book) || t(error.fallbackKey)
  }
}
