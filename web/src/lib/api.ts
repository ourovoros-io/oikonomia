import { invoke } from '@tauri-apps/api/core'
import type { LicenseStatus } from './license'
import { isTauri, type CommandError } from './tauri'

export type { LicenseState, LicenseStatus } from './license'

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
  hidden: boolean
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

/** The Expense account with the most spending in a dashboard window. */
export type TopExpense = {
  code: string
  name: string
  amount_minor: number
  /** Share of the window's expenses, in basis points. */
  share_bps: number
}

export type DashboardSummary = {
  entity_id: string
  base_currency: string
  cash_like_assets: number
  income: number
  expenses: number
  net_income: number
  recent_entry_count: number
  /** Net as a share of income, in basis points; null when income is zero or less. */
  savings_rate_bps: number | null
  /** Expenses as a share of income, in basis points; null when income is zero or less. */
  spend_ratio_bps: number | null
  /** Largest Expense account in the window; null when there are no expenses. */
  top_expense: TopExpense | null
  /** Net against the previous period, in basis points of its size; null when that net is zero. */
  net_vs_previous_bps: number | null
}

export type CashFlowGranularity = 'day' | 'month'

export type CashFlowBucket = {
  start: string
  end: string
  income_minor: number
  expenses_minor: number
  cumulative_income_minor: number
  cumulative_expenses_minor: number
}

/** Income and expenses per day or month, computed in Rust on the dashboard's basis. */
export type CashFlowSeries = {
  entity_id: string
  from: string
  to: string
  granularity: CashFlowGranularity
  total_income_minor: number
  total_expenses_minor: number
  net_minor: number
  buckets: CashFlowBucket[]
}

export type CreateJournalLine = {
  account_id: string
  debit_minor: number
  credit_minor: number
  memo?: string | null
}

/** One line in an account register, with a running normal balance. */
export type RegisterLine = {
  entry_id: string
  entry_date: string
  description: string
  debit_minor: number
  credit_minor: number
  balance_minor: number
  hidden: boolean
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

/** UI language, persisted on UiPrefs (plaintext, readable before unlock). */
export type Locale = 'en' | 'el' | 'fr' | 'de'

/** Last role-account picks for a single entity+kind tray post (snake_case matches Rust). */
export type LastRoleAccounts = {
  category_account_id: string | null
  wallet_account_id: string | null
  payable_account_id: string | null
  from_account_id: string | null
  to_account_id: string | null
}

/** Full plaintext UI prefs (tray last-used + locale). Safe before unlock. */
export type UiPrefs = {
  last_entity_id: string | null
  last_accounts_by_entity_kind: Record<string, LastRoleAccounts>
  /** Absent on older prefs files; treat as `en`. */
  locale?: Locale
}

/** Simple-form posting input; the kind → debit/credit mapping lives in Rust. */
export type SimpleEntryKind = 'expense' | 'income' | 'bill' | 'transfer'

export type SimpleEntryInput = {
  entity_id: string
  kind: SimpleEntryKind
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

/** Same kinds as journal entries — a template is a recipe, not a new type. */
export type RecurringKind = SimpleEntryKind

/** Cadence is a closed set. Monthly is the v1 default; Weekly/Yearly are available. */
export type RecurringCadence = 'monthly' | 'weekly' | 'yearly'

/**
 * Recurring template as returned by Rust (`RecurringTemplateView`).
 * `due` is `next_date` on/before today (UTC) — Rust defines that.
 */
export type RecurringTemplate = {
  id: string
  entity_id: string
  name: string
  kind: RecurringKind
  bill_status: 'paid' | 'unpaid' | 'pay_existing' | null
  amount_minor: number
  cadence: RecurringCadence
  day_of_month: number | null
  category_account_id: string | null
  wallet_account_id: string | null
  payable_account_id: string | null
  from_account_id: string | null
  to_account_id: string | null
  memo: string | null
  next_date: string
  due: boolean
}

/** Create body. Matches `CreateRecurringTemplate`. Rust assigns id and due. */
export type RecurringTemplateInput = {
  entity_id: string
  name: string
  kind: RecurringKind
  bill_status: 'paid' | 'unpaid' | 'pay_existing' | null
  amount_minor: number
  cadence: RecurringCadence
  day_of_month: number | null
  category_account_id: string | null
  wallet_account_id: string | null
  payable_account_id: string | null
  from_account_id: string | null
  to_account_id: string | null
  memo: string | null
  next_date: string
}

/** Update body. Matches `UpdateRecurringTemplate` (`entity_id` is immutable). */
export type RecurringTemplateUpdate = Omit<RecurringTemplateInput, 'entity_id'> & {
  id: string
}

/** `recurring_post` result: one journal entry plus the advanced template. */
export type RecurringPostResult = {
  entry: PostedEntryView
  template: RecurringTemplate
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
  /** Owner-only hidden flag. Hidden rows stay in list/get; CSV export omits them. */
  entrySetHidden: (id: string, hidden: boolean) =>
    call<PostedEntryView>('entry_set_hidden', { id, hidden }),
  /** Edit = void + repost in one transaction; documents follow the new entry. */
  entryReplaceSimple: (originalId: string, input: SimpleEntryInput) =>
    call<PostedEntryView>('entry_replace_simple', { originalId, input }),

  reportTrialBalance: (entityId: string, asOf: string) =>
    call<TrialBalance>('report_trial_balance', { entityId, asOf }),
  reportPnl: (entityId: string, from: string, to: string) =>
    call<PnL>('report_pnl', { entityId, from, to }),
  /** Accountant / PDF P&L. Same args as `reportPnl`; Hidden omitted. */
  reportPnlExport: (entityId: string, from: string, to: string) =>
    call<PnL>('report_pnl_export', { entityId, from, to }),
  reportBalanceSheet: (entityId: string, asOf: string) =>
    call<BalanceSheet>('report_balance_sheet', { entityId, asOf }),
  /**
   * Native Save for a generated monthly-expenses PDF. `null` = cancelled.
   * Bytes stay local; Rust opens the dialog and writes the file.
   */
  reportExportPdf: (input: { bytesBase64: string; suggestedName?: string }) =>
    call<string | null>('report_export_pdf', {
      bytesBase64: input.bytesBase64,
      suggestedName: input.suggestedName ?? null,
    }),
  dashboardSummary: (entityId: string, from: string, to: string, assetsAsOf: string) =>
    call<DashboardSummary>('dashboard_summary_cmd', { entityId, from, to, assetsAsOf }),
  /**
   * The cash-flow light's data. A null bound lets Rust resolve it to the
   * book's first or last active entry.
   */
  cashFlowSeries: (entityId: string, from: string | null, to: string | null) =>
    call<CashFlowSeries>('cash_flow_series_cmd', { entityId, from, to }),

  /** Signed normal balance of one account as of an ISO date. */
  accountBalance: (accountId: string, asOf: string) =>
    call<number>('account_balance_cmd', { accountId, asOf }),
  /** Register lines (with running balance) for one account, optionally date-bounded. */
  accountRegister: (accountId: string, from?: string, to?: string) =>
    call<RegisterLine[]>('account_register_cmd', {
      accountId,
      from: from ?? null,
      to: to ?? null,
    }),
  /** Set the account's actual balance; Rust posts the delta against equity. */
  accountSetOpeningBalance: (accountId: string, targetMinor: number, asOf: string) =>
    call<PostedEntryView>('account_set_opening_balance', { accountId, targetMinor, asOf }),

  getLockTimeout: () => call<number>('settings_get_lock_timeout'),
  setLockTimeout: (secs: number) => call<void>('settings_set_lock_timeout', { secs }),
  /** Trial / signed-license state. Readable while unlocked. */
  licenseStatus: () => call<LicenseStatus>('license_status'),
  /**
   * Native Open for a `.lic` file. `null` = cancelled.
   * Returns the same status object as {@link api.licenseStatus} after install.
   */
  licenseInstall: () => call<LicenseStatus | null>('license_install'),
  /**
   * Bundled EULA text for the Settings license viewer. Empty (not an
   * error) outside the desktop app, since it is purely informational chrome.
   */
  eulaText: () => (isTauri() ? call<string>('eula_text') : Promise.resolve('')),
  /** Opens the mail client on the support mailbox. Rust builds the mailto; nothing is passed in. */
  openSupportEmail: () => call<void>('open_support_email'),
  /** Locale is a plaintext pref (Rust side): readable before unlock. */
  getLocale: () => call<Locale>('settings_get_locale'),
  setLocale: (locale: Locale) => call<void>('settings_set_locale', { locale }),
  /** Full plaintext UI prefs (tray last-used + locale). Safe before unlock. */
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

  /**
   * Parse a bank CSV into suggested simple entries. Does not post.
   * Omit `path` so Rust shows a native Open dialog. Omit `mapping` to auto-detect.
   * `null` = cancelled.
   */
  csvImportPreview: (input: {
    entity_id: string
    path?: string | null
    wallet_account_id?: string | null
    expense_account_id?: string | null
    income_account_id?: string | null
    mapping?: CsvColumnMapping | null
  }) => {
    const payload: {
      entity_id: string
      path?: string
      wallet_account_id?: string | null
      expense_account_id?: string | null
      income_account_id?: string | null
      mapping?: CsvColumnMapping
    } = {
      entity_id: input.entity_id,
      wallet_account_id: input.wallet_account_id ?? null,
      expense_account_id: input.expense_account_id ?? null,
      income_account_id: input.income_account_id ?? null,
    }
    if (input.path) {
      payload.path = input.path
    }
    if (input.mapping) {
      payload.mapping = input.mapping
    }
    return call<CsvImportPreview | null>('csv_import_preview', { input: payload })
  },
  /** Post selected preview rows. Duplicates are skipped unless `include_duplicates`. */
  csvImportPost: (input: { rows: SimpleEntryInput[]; include_duplicates?: boolean }) =>
    call<CsvImportPostResult>('csv_import_post', { input }),
  /** Native Save dialog. `null` = cancelled. Writes an unencrypted accountant CSV. */
  csvExportJournal: (entityId: string) =>
    call<string | null>('csv_export_journal', { entityId }),

  /**
   * Recurring templates are recipes, not a second ledger.
   * `due` is Rust-owned (`next_date` on/before today) — the UI must not recompute it.
   * Commands: recurring_list / create / update / delete / post.
   */
  recurringList: (entityId: string) => call<RecurringTemplate[]>('recurring_list', { entityId }),
  recurringGet: (id: string) => call<RecurringTemplate>('recurring_get', { id }),
  recurringCreate: (input: RecurringTemplateInput) =>
    call<RecurringTemplate>('recurring_create', { input }),
  recurringUpdate: (input: RecurringTemplateUpdate) =>
    call<RecurringTemplate>('recurring_update', { input }),
  recurringDelete: (id: string) => call<void>('recurring_delete', { id }),
  /**
   * Manual post → one journal entry via the existing simple-entry path.
   * `entry_date` / `amount_minor` are optional overrides for “adjust before saving”.
   */
  recurringPost: (
    id: string,
    opts?: { entry_date?: string | null; amount_minor?: number | null },
  ) =>
    call<RecurringPostResult>('recurring_post', {
      id,
      entryDate: opts?.entry_date ?? null,
      amountMinor: opts?.amount_minor ?? null,
    }),
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

/** Header-name mapping for one bank CSV. Values are header names (ASCII case-insensitive). */
export type CsvColumnMapping = {
  date?: string | null
  description?: string | null
  /** XOR debit+credit. */
  amount?: string | null
  debit?: string | null
  credit?: string | null
  reference?: string | null
}

/** Preview of a bank CSV. Does not write to the ledger. */
export type CsvImportPreview = {
  source: string
  /** Present once Rust returns the header row (Map columns selects). */
  headers?: string[]
  /** Auto-detect pre-fill; present even when the caller passed `mapping`. */
  detected_mapping?: CsvColumnMapping
  rows: CsvImportPreviewRow[]
}

export type CsvImportPreviewRow = {
  source_row: number
  duplicate: boolean
  error: string | null
  suggested: SimpleEntryInput | null
  signed_amount_minor: number | null
}

export type CsvImportPostResult = {
  posted: PostedEntryView[]
  skipped_duplicate_count: number
}

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
