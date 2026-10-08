/**
 * Form choices that outlive the page that made them, for this session only.
 * Pages are torn down on every navigation, so component state alone would
 * reset the New entity currency to EUR each time the form opens.
 */
const DEFAULT_CURRENCY = 'EUR'

let lastBookCurrency = DEFAULT_CURRENCY

/** The currency last picked in New entity, or EUR before any pick. */
export function rememberedBookCurrency(): string {
  return lastBookCurrency
}

export function rememberBookCurrency(code: string): void {
  lastBookCurrency = code
}

export function resetSessionDefaults(): void {
  lastBookCurrency = DEFAULT_CURRENCY
}
