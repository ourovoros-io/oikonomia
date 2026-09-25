import type { AnalyzerStatus, UiPrefs } from '../../src/lib/api'
import type { LicenseStatus } from '../../src/lib/license'
import { buildLedger, type DemoLang } from './ledger'
import {
  accountBalance,
  accountRegister,
  balanceSheet,
  cashFlowSeries,
  dashboardSummary,
  entryList,
  pnl,
  trialBalance,
} from './views'

type Args = Record<string, unknown>

const str = (a: Args, k: string) => a[k] as string
const opt = (a: Args, k: string) => (a[k] ?? null) as string | null

/**
 * Answers every read the four captured screens make. Writes and native
 * dialogs are deliberately absent: a capture that reaches one is wrong.
 */
export function createHandler(lang: DemoLang) {
  const l = buildLedger(lang)

  const license: LicenseStatus = { state: 'licensed', licensed_until: '2027-09-24' }
  const prefs: UiPrefs = { last_entity_id: l.entity.id, last_accounts_by_entity_kind: {}, locale: lang }
  const analyzer: AnalyzerStatus = { ocr_available: true, offline: true, hint: '' }

  const answers: Record<string, (a: Args) => unknown> = {
    vault_status: () => 'unlocked',
    vault_touch: () => null,
    app_info: () => ({ name: 'Oikonomia', version: '1.0.0', support_email: 'support@example.com' }),
    update_check: () => ({ kind: 'up_to_date' }),
    eula_text: () => '',
    license_status: () => license,
    settings_get_locale: () => lang,
    settings_set_locale: () => null,
    settings_get_ui_prefs: () => prefs,
    settings_get_lock_timeout: () => 900,
    entity_list: () => [l.entity],
    account_list: () => l.accounts,
    entry_list: (a) =>
      entryList(l, { from: opt(a, 'from'), to: opt(a, 'to'), search: opt(a, 'search'), accountId: opt(a, 'accountId') }),
    dashboard_summary_cmd: (a) => dashboardSummary(l, str(a, 'from'), str(a, 'to'), str(a, 'assetsAsOf')),
    cash_flow_series_cmd: (a) => cashFlowSeries(l, opt(a, 'from'), opt(a, 'to')),
    report_pnl: (a) => pnl(l, str(a, 'from'), str(a, 'to')),
    report_pnl_export: (a) => pnl(l, str(a, 'from'), str(a, 'to')),
    report_trial_balance: (a) => trialBalance(l, str(a, 'asOf')),
    report_balance_sheet: (a) => balanceSheet(l, str(a, 'asOf')),
    account_balance_cmd: (a) => accountBalance(l, str(a, 'accountId'), str(a, 'asOf')),
    account_register_cmd: (a) => accountRegister(l, str(a, 'accountId'), opt(a, 'from'), opt(a, 'to')),
    document_list: () => l.documents,
    document_analyzer_status: () => analyzer,
    recurring_list: () => l.recurring,
  }

  return (cmd: string, args: Args = {}): unknown => {
    const answer = answers[cmd]

    if (!answer) {
      console.error('[marketing] missing command', cmd)
      throw new Error(`marketing fixture has no answer for ${cmd}`)
    }

    return answer(args)
  }
}
