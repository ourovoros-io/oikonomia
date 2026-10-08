/** The dates a user chose in Reports. */
export type ReportPeriod = {
  /** First day of the profit and loss window, `YYYY-MM-DD`. */
  from: string
  /** Last day of the profit and loss window, `YYYY-MM-DD`. */
  to: string
  /** The as-of day of the trial balance and the balance sheet. */
  asOf: string
}

// Reports remounts on every visit, so the choice lives here, for as long as
// the app is open. It is deliberately not saved to disk: a new session starts
// on the current month again.
let remembered: Partial<ReportPeriod> = {}

/** The dates chosen earlier in this session; a date not chosen yet is absent. */
export function rememberedReportPeriod(): Partial<ReportPeriod> {
  return remembered
}

/** Keeps the dates in `chosen`, ignoring a blank one: a date field mid-edit is empty. */
export function rememberReportPeriod(chosen: Partial<ReportPeriod>): void {
  const complete = Object.fromEntries(Object.entries(chosen).filter(([, date]) => date))
  remembered = { ...remembered, ...complete }
}

/** Forgets every chosen date, so a test starts from the defaults. */
export function forgetReportPeriodForTests(): void {
  remembered = {}
}
