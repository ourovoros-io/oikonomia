import type { AnalyzerStatus, UiPrefs } from '../../src/lib/api'
import { accountDefaults, buildLedger, type DemoLang } from './ledger'
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

/**
 * Answers every read the four captured screens make, plus the two no-op
 * writes -- `vault_touch` and `settings_set_locale` -- the app itself calls
 * on load, and `settings_resolve_locale`, which returns the demo language
 * the capture was asked for. Anything beyond those reads and no-op writes, a real write or a
 * native dialog, is deliberately unanswered: a capture that reaches one is
 * wrong.
 */
export function createHandler(lang: DemoLang) {
  const l = buildLedger(lang)

  const prefs: UiPrefs = { last_entity_id: l.entity.id, last_accounts_by_entity_kind: {}, locale: lang }
  const analyzer: AnalyzerStatus = { ocr_available: true, offline: true, hint: 'ready' }

  // Set to the command currently being answered so str/opt can name it in a
  // failure, without every call site having to repeat the command string.
  let currentCmd = ''

  const missingArg = (k: string): never => {
    console.error('[marketing] missing arg', currentCmd, k)
    throw new Error(`marketing fixture: ${currentCmd} missing arg ${k}`)
  }

  const str = (a: Args, k: string): string => {
    const v = a[k]
    return typeof v === 'string' ? v : missingArg(k)
  }

  const opt = (a: Args, k: string): string | null => {
    const v = a[k] ?? null
    return v === null || typeof v === 'string' ? v : missingArg(k)
  }

  const answers: Record<string, (a: Args) => unknown> = {
    vault_status: () => 'unlocked',
    vault_touch: () => null,
    app_info: () => ({ name: 'Oikonomia', version: '0.1.0', support_email: 'info@ourovoros.io' }),
    donation_addresses: () => [],
    update_check: () => ({ kind: 'up_to_date' }),
    settings_get_locale: () => lang,
    settings_resolve_locale: () => lang,
    settings_set_locale: () => null,
    settings_get_ui_prefs: () => prefs,
    settings_get_lock_timeout: () => 900,
    entity_list: () => [l.entity],
    account_list: () => l.accounts,
    account_defaults: () => accountDefaults(),
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

    currentCmd = cmd
    return answer(args)
  }
}
