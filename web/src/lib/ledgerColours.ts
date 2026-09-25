/**
 * The Ledger light: the gradient stops every money mark is painted with. The
 * same six values are the --color-money-{in,out}-{a,b,c} tokens in index.css.
 */

/** Money in, from the lit end to the deep end. */
export const LEDGER_IN_STOPS = ['#27bf93', '#1ba39a', '#1e8db0'] as const
/** Money out, from the lit end to the deep end. */
export const LEDGER_OUT_STOPS = ['#f07a45', '#e8603f', '#d64a5a'] as const
