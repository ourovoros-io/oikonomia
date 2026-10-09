import type { Locale } from './i18n'

/**
 * Updater-dialog button widths, in CSS pixels, one pair per locale.
 *
 * Designer fixed these so Cancel, Close and Later share a width and
 * "Install and restart" keeps its full label. They are applied as inline
 * widths, not measured at runtime, so a font load cannot move the row.
 * The row is right-aligned; the gap between the two buttons is 8.
 */
export const UPDATE_DIALOG_BUTTON_WIDTHS: Record<
  Locale,
  { secondary: number; primary: number }
> = {
  en: { secondary: 96, primary: 144 },
  fr: { secondary: 96, primary: 176 },
  el: { secondary: 96, primary: 232 },
  de: { secondary: 104, primary: 208 },
}
