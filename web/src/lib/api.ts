import { invoke } from '@tauri-apps/api/core'
import { isTauri, type CommandError } from './tauri'

export type ChartTemplate = 'personal' | 'company' | 'blank'
export type AccountType = 'asset' | 'liability' | 'equity' | 'income' | 'expense'

export type Entity = {
  id: string
  name: string
  base_currency: string
  fiscal_year_start_month: number
  chart_template: ChartTemplate
}

export type Account = {
  id: string
  entity_id: string
  code: string
  name: string
  account_type: AccountType
  parent_id: string | null
  is_active: boolean
  is_system: boolean
  sort_order: number
}

export type Money = { amount_minor: number }

export type JournalLine = {
  id: string
  entry_id: string
  account_id: string
  debit: Money
  credit: Money
  memo: string | null
}

export type JournalEntry = {
  id: string
  entity_id: string
  entry_date: string
  description: string
  reference: string | null
  status: 'draft' | 'posted'
}

export type PostedEntryView = {
  entry: JournalEntry
  lines: JournalLine[]
  is_voided: boolean
}

export type ReportLine = {
  code: string
  name: string
  account_type: AccountType
  debit_minor: number
  credit_minor: number
  balance_minor: number
}

export type TrialBalance = {
  entity_id: string
  as_of: string
  lines: ReportLine[]
  total_debits: number
  total_credits: number
}

export type PnL = {
  entity_id: string
  from: string
  to: string
  income: ReportLine[]
  expenses: ReportLine[]
  total_income: number
  total_expenses: number
  net_income: number
}

export type BalanceSheetSection = {
  title: string
  lines: ReportLine[]
  total: number
}

export type BalanceSheet = {
  entity_id: string
  as_of: string
  assets: BalanceSheetSection
  liabilities: BalanceSheetSection
  equity: BalanceSheetSection
  total_assets: number
  total_liabilities_equity: number
}

export type DashboardSummary = {
  entity_id: string
  base_currency: string
  cash_like_assets: number
  income: number
  expenses: number
  net_income: number
  recent_entry_count: number
}

export type CreateJournalLine = {
  account_id: string
  debit_minor: number
  credit_minor: number
  memo?: string | null
}

function asCommandError(err: unknown): CommandError {
  if (typeof err === 'string') {
    return { code: 'unknown', message: err }
  }
  if (err && typeof err === 'object') {
    const obj = err as Record<string, unknown>
    // Tauri often wraps payload as { message, code } or { error, ... }
    if (typeof obj.message === 'string') {
      return {
        code: typeof obj.code === 'string' ? obj.code : 'unknown',
        message: obj.message,
      }
    }
    if (typeof obj.error === 'string') {
      return { code: 'unknown', message: obj.error }
    }
  }
  if (err instanceof Error) {
    return { code: 'unknown', message: err.message }
  }
  return {
    code: 'unknown',
    message: String(err),
  }
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw asCommandError(new Error(`${cmd} requires the desktop app`))
  }
  try {
    return await invoke<T>(cmd, args)
  } catch (err) {
    throw asCommandError(err)
  }
}

/** UI color theme, persisted outside the encrypted vault. */
export type Theme = 'dark' | 'light'

/** Last role-account picks for a single entity+kind tray post (snake_case matches Rust). */
export type LastRoleAccounts = {
  category_account_id: string | null
  wallet_account_id: string | null
  payable_account_id: string | null
  from_account_id: string | null
  to_account_id: string | null
}

/** Full plaintext UI prefs (theme + tray last-used). Safe before unlock. */
export type UiPrefs = {
  theme: Theme
  last_entity_id: string | null
  last_accounts_by_entity_kind: Record<string, LastRoleAccounts>
}

/** Simple-form posting input; the kind → debit/credit mapping lives in Rust. */
export type SimpleEntryInput = {
  entity_id: string
  kind: 'expense' | 'income' | 'bill' | 'transfer'
  bill_status: 'paid' | 'unpaid' | 'pay_existing' | null
  entry_date: string
  description: string
  reference: string | null
  amount_minor: number
  category_account_id: string | null
  wallet_account_id: string | null
  payable_account_id: string | null
  from_account_id: string | null
  to_account_id: string | null
}

export const api = {
  entityList: () => call<Entity[]>('entity_list'),
  entityCreate: (input: {
    name: string
    base_currency: string
    chart_template: ChartTemplate
    fiscal_year_start_month?: number | null
  }) => call<Entity>('entity_create', { input }),
  entityUpdate: (id: string, name: string) => call<Entity>('entity_update', { id, name }),
  entityArchive: (id: string) => call<void>('entity_archive', { id }),
  entityDelete: (id: string) => call<void>('entity_delete', { id }),

  accountList: (entityId: string) => call<Account[]>('account_list', { entityId }),
  accountCreate: (input: {
    entity_id: string
    code: string
    name: string
    account_type: AccountType
    sort_order?: number | null
  }) => call<Account>('account_create', { input }),
  accountUpdate: (input: {
    id: string
    code: string
    name: string
    is_active: boolean
    sort_order: number
  }) => call<Account>('account_update', { input }),
  accountArchive: (id: string) => call<void>('account_archive', { id }),

  entryList: (
    entityId: string,
    opts?: { from?: string; to?: string; search?: string; accountId?: string },
  ) =>
    call<PostedEntryView[]>('entry_list', {
      entityId,
      from: opts?.from ?? null,
      to: opts?.to ?? null,
      search: opts?.search ?? null,
      accountId: opts?.accountId ?? null,
    }),
  entryPost: (input: {
    entity_id: string
    entry_date: string
    description: string
    reference?: string | null
    lines: CreateJournalLine[]
  }) => call<PostedEntryView>('entry_post', { input }),
  /** Simple-form posting: the kind → debit/credit mapping lives in Rust. */
  entryPostSimple: (input: SimpleEntryInput) => call<PostedEntryView>('entry_post_simple', { input }),
  /** Post a simple entry together with its analyzed document (one transaction). */
  entryPostSimpleWithDocument: (
    input: SimpleEntryInput,
    doc: { filename: string; mimeType: string; dataBase64: string },
    analysisJson?: string,
  ) =>
    call<PostedEntryView>('entry_post_simple_with_document', {
      input,
      filename: doc.filename,
      mimeType: doc.mimeType,
      dataBase64: doc.dataBase64,
      analysisJson: analysisJson ?? null,
    }),
  /** Same, for native drops: the backend re-reads the path at post time. */
  entryPostSimpleWithDocumentPath: (
    input: SimpleEntryInput,
    path: string,
    analysisJson?: string,
  ) =>
    call<PostedEntryView>('entry_post_simple_with_document_path', {
      input,
      path,
      analysisJson: analysisJson ?? null,
    }),
  entryVoid: (id: string) => call<{ original_id: string; reverse_id: string }>('entry_void', { id }),
  /** Edit = void + repost in one transaction; documents follow the new entry. */
  entryReplaceSimple: (originalId: string, input: SimpleEntryInput) =>
    call<PostedEntryView>('entry_replace_simple', { originalId, input }),

  reportTrialBalance: (entityId: string, asOf: string) =>
    call<TrialBalance>('report_trial_balance', { entityId, asOf }),
  reportPnl: (entityId: string, from: string, to: string) =>
    call<PnL>('report_pnl', { entityId, from, to }),
  reportBalanceSheet: (entityId: string, asOf: string) =>
    call<BalanceSheet>('report_balance_sheet', { entityId, asOf }),
  dashboardSummary: (entityId: string, from: string, to: string, assetsAsOf: string) =>
    call<DashboardSummary>('dashboard_summary_cmd', { entityId, from, to, assetsAsOf }),

  /** Signed normal balance of one account as of an ISO date. */
  accountBalance: (accountId: string, asOf: string) =>
    call<number>('account_balance_cmd', { accountId, asOf }),
  /** Set the account's actual balance; Rust posts the delta against equity. */
  accountSetOpeningBalance: (accountId: string, targetMinor: number, asOf: string) =>
    call<PostedEntryView>('account_set_opening_balance', { accountId, targetMinor, asOf }),

  getLockTimeout: () => call<number>('settings_get_lock_timeout'),
  setLockTimeout: (secs: number) => call<void>('settings_set_lock_timeout', { secs }),
  /** Theme is a plaintext pref (Rust side): readable before unlock. */
  getTheme: () => call<Theme>('settings_get_theme'),
  setTheme: (theme: Theme) => call<void>('settings_set_theme', { theme }),
  /** Full plaintext UI prefs (theme + tray last-used). Safe before unlock. */
  getUiPrefs: () => call<UiPrefs>('settings_get_ui_prefs'),
  /** Remember last entity + role accounts after a successful tray post. */
  rememberQuickAdd: (entityId: string, kind: string, accounts: LastRoleAccounts) =>
    call<void>('settings_remember_quick_add', {
      entityId,
      kind,
      accounts,
    }),
  openMainWindow: () => call<void>('open_main_window'),
  quickAddHide: () => call<void>('quick_add_hide'),

  documentAnalyzerStatus: () => call<AnalyzerStatus>('document_analyzer_status'),
  documentAnalyze: (input: {
    entityId: string
    filename: string
    mimeType: string
    dataBase64: string
  }) =>
    call<DocumentSuggestion>('document_analyze', {
      entityId: input.entityId,
      filename: input.filename,
      mimeType: input.mimeType,
      dataBase64: input.dataBase64,
    }),
  /** Native Tauri drop: filesystem path (Finder drops often omit File objects). */
  documentAnalyzePath: (input: { entityId: string; path: string }) =>
    call<DocumentSuggestion>('document_analyze_path', {
      entityId: input.entityId,
      path: input.path,
    }),
  documentList: (entityId: string) => call<DocumentMeta[]>('document_list', { entityId }),
  documentGet: (documentId: string) => call<DocumentContent>('document_get', { documentId }),
  documentDelete: (documentId: string) => call<void>('document_delete', { documentId }),
  documentAttach: (input: {
    entityId: string
    entryId: string
    filename: string
    mimeType: string
    dataBase64: string
  }) => call<DocumentMeta>('document_attach', input),
  documentExport: (documentId: string) => call<string | null>('document_export', { documentId }),
}

export type AnalyzerStatus = {
  ocr_available: boolean
  offline: boolean
  hint: string
}

export type DocumentSuggestion = {
  source: 'bundled_ocr' | 'heuristic' | 'none'
  model: string | null
  kind: 'expense' | 'income' | 'bill'
  amount_minor: number | null
  entry_date: string | null
  description: string | null
  reference: string | null
  merchant: string | null
  bill_unpaid: boolean
  category_account_id: string | null
  wallet_account_id: string | null
  payable_account_id: string | null
  confidence: number
  notes: string
}

export type DocumentMeta = {
  id: string
  entity_id: string
  entry_id: string
  filename: string
  mime_type: string
  size_bytes: number
  created_at: string
  entry_description: string
}

export type DocumentContent = {
  meta: DocumentMeta
  data_base64: string
}

/** Where a pending (not yet saved) document lives until the entry is posted. */
export type PendingDocSource =
  | { kind: 'file'; file: File }
  | { kind: 'path'; path: string }

export { formatMoney, formatDate, isoDate, parseMajorToMinor, localeForCurrency } from './money'

export function todayISO(): string {
  const d = new Date()
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

export function monthStartISO(): string {
  const d = new Date()
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  return `${y}-${m}-01`
}

export function monthEndISO(): string {
  const d = new Date()
  const last = new Date(d.getFullYear(), d.getMonth() + 1, 0)
  const m = String(last.getMonth() + 1).padStart(2, '0')
  return `${last.getFullYear()}-${m}-${String(last.getDate()).padStart(2, '0')}`
}

export function yearStartISO(): string {
  return `${new Date().getFullYear()}-01-01`
}

export function yearEndISO(): string {
  return `${new Date().getFullYear()}-12-31`
}
