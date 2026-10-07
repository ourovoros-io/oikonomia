//! The JSON form of the ledger, report, CSV, document and preference types that cross
//! IPC, pinned as literal text.
//!
//! The web UI mirrors these shapes by hand in `web/src/lib/api.ts`, so a
//! change to a field name, to nesting or to the type of a value breaks the
//! UI without breaking any Rust build. Each test here reads one literal into
//! the Rust type and writes it back, and the text must come out unchanged:
//! that pins the field names, their order, how dates, ids and amounts are
//! written, and that the type still reads what it writes.
//!
//! A refactor of the types behind the wire must leave every literal in this
//! file as it is. Only the name of the Rust type a literal is read into may
//! change.

#![expect(
    clippy::expect_used,
    reason = "a test helper states why each step cannot fail"
)]

use oikonomia_core::csv::{
    CsvImportPostInput, CsvImportPostResult, CsvImportPreview, CsvImportPreviewRow, JournalCsvLine,
};
use oikonomia_core::documents::{AnalyzerStatus, DocumentMeta, DocumentSuggestion};
use oikonomia_core::domain::{Account, Entity, JournalEntry, JournalLine};
use oikonomia_core::ledger::{
    BalanceSheet, CashFlowSeries, CreateAccount, CreateEntity, CreateRecurringTemplateRequest,
    DashboardSummary, JournalLineRequest, PnL, PostJournalRequest, PostSimpleEntryRequest,
    PostedEntryView, RecurringPostResult, RecurringTemplateView, RegisterLine, TrialBalance,
    UpdateAccount, UpdateRecurringTemplateRequest, VoidResult,
};
use oikonomia_core::prefs::{UiPrefs, load_ui_prefs_view, ui_prefs_path};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Reads `json` as a `T` and checks that writing the value gives `json` back.
#[track_caller]
fn assert_json_is_pinned<T: Serialize + DeserializeOwned>(json: &str) {
    let value: T = serde_json::from_str(json).expect("the pinned JSON reads as the type");
    let written = serde_json::to_string(&value).expect("the value serializes");

    assert_eq!(written, json);
}

/// Returns whether the JSON layer refuses to read `json` as a `T`.
fn is_refused<T: DeserializeOwned>(json: &str) -> bool {
    serde_json::from_str::<T>(json).is_err()
}

#[test]
fn entity() {
    assert_json_is_pinned::<Entity>(concat!(
        r#"{"id":"11111111-1111-4111-8111-111111111111","name":"Home","base_currency":"EUR","#,
        r#""base_currency_decimals":2,"fiscal_year_start_month":4,"chart_template":"personal"}"#,
    ));
}

/// The number of decimals is core's, for a currency with none and for one
/// with three as much as for the usual two.
#[test]
fn entity_carries_the_decimals_of_its_currency() {
    assert_json_is_pinned::<Entity>(concat!(
        r#"{"id":"11111111-1111-4111-8111-111111111111","name":"Tokyo","base_currency":"JPY","#,
        r#""base_currency_decimals":0,"fiscal_year_start_month":4,"chart_template":"personal"}"#,
    ));
    assert_json_is_pinned::<Entity>(concat!(
        r#"{"id":"11111111-1111-4111-8111-111111111111","name":"Kuwait","base_currency":"KWD","#,
        r#""base_currency_decimals":3,"fiscal_year_start_month":1,"chart_template":"company"}"#,
    ));
}

#[test]
fn create_entity() {
    assert_json_is_pinned::<CreateEntity>(concat!(
        r#"{"name":"Acme","base_currency":"USD","chart_template":"company","#,
        r#""fiscal_year_start_month":null}"#,
    ));
    assert_json_is_pinned::<CreateEntity>(concat!(
        r#"{"name":"Acme","base_currency":"USD","chart_template":"blank","#,
        r#""fiscal_year_start_month":12}"#,
    ));
}

#[test]
fn account() {
    assert_json_is_pinned::<Account>(concat!(
        r#"{"id":"22222222-2222-4222-8222-222222222222","entity_id":"#,
        r#""11111111-1111-4111-8111-111111111111","code":"1000","name":"Cash","account_type":"#,
        r#""asset","is_active":true,"is_system":false,"sort_order":10}"#,
    ));
}

#[test]
fn create_and_update_account() {
    assert_json_is_pinned::<CreateAccount>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","code":"6100","name":"Rent","#,
        r#""account_type":"expense","sort_order":null}"#,
    ));
    assert_json_is_pinned::<UpdateAccount>(concat!(
        r#"{"id":"22222222-2222-4222-8222-222222222222","code":"6100","name":"Rent","#,
        r#""is_active":false,"sort_order":3}"#,
    ));
}

#[test]
fn journal_entry() {
    assert_json_is_pinned::<JournalEntry>(concat!(
        r#"{"id":"33333333-3333-4333-8333-333333333333","entity_id":"#,
        r#""11111111-1111-4111-8111-111111111111","entry_date":"2026-08-10","description":"#,
        r#""Groceries","reference":"INV-7","status":"posted","hidden":false}"#,
    ));
}

#[test]
fn an_entry_whose_status_is_not_posted_is_refused_by_the_json_layer() {
    assert!(is_refused::<JournalEntry>(concat!(
        r#"{"id":"33333333-3333-4333-8333-333333333333","entity_id":"#,
        r#""11111111-1111-4111-8111-111111111111","entry_date":"2026-08-10","description":"#,
        r#""Groceries","reference":"INV-7","status":"draft","hidden":false}"#,
    )));
}

#[test]
fn journal_line_on_each_side() {
    assert_json_is_pinned::<JournalLine>(concat!(
        r#"{"id":"44444444-4444-4444-8444-444444444444","entry_id":"#,
        r#""33333333-3333-4333-8333-333333333333","account_id":"#,
        r#""22222222-2222-4222-8222-222222222222","debit":{"amount_minor":4500},"credit":{"#,
        r#""amount_minor":0},"memo":null}"#,
    ));
    assert_json_is_pinned::<JournalLine>(concat!(
        r#"{"id":"44444444-4444-4444-8444-444444444444","entry_id":"#,
        r#""33333333-3333-4333-8333-333333333333","account_id":"#,
        r#""22222222-2222-4222-8222-222222222222","debit":{"amount_minor":0},"credit":{"#,
        r#""amount_minor":4500},"memo":"card"}"#,
    ));
}

#[test]
fn posted_entry_view() {
    assert_json_is_pinned::<PostedEntryView>(concat!(
        r#"{"entry":{"id":"33333333-3333-4333-8333-333333333333","entity_id":"#,
        r#""11111111-1111-4111-8111-111111111111","entry_date":"2026-08-10","description":"#,
        r#""Groceries","reference":null,"status":"posted","hidden":true},"lines":[{"id":"#,
        r#""44444444-4444-4444-8444-444444444444","entry_id":"#,
        r#""33333333-3333-4333-8333-333333333333","account_id":"#,
        r#""22222222-2222-4222-8222-222222222222","debit":{"amount_minor":4500},"credit":{"#,
        r#""amount_minor":0},"memo":null},{"id":"55555555-5555-4555-8555-555555555555","#,
        r#""entry_id":"33333333-3333-4333-8333-333333333333","account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","debit":{"amount_minor":0},"credit":{"#,
        r#""amount_minor":4500},"memo":null}],"is_voided":false}"#,
    ));
}

#[test]
fn register_line() {
    assert_json_is_pinned::<RegisterLine>(concat!(
        r#"{"entry_id":"33333333-3333-4333-8333-333333333333","entry_date":"2026-08-10","#,
        r#""description":"Groceries","debit_minor":0,"credit_minor":4500,"balance_minor":"#,
        r#"-4500,"hidden":false}"#,
    ));
}

#[test]
fn void_result() {
    assert_json_is_pinned::<VoidResult>(concat!(
        r#"{"original_id":"33333333-3333-4333-8333-333333333333","reverse_id":"#,
        r#""77777777-7777-4777-8777-777777777777"}"#,
    ));
}

#[test]
fn post_journal_request() {
    assert_json_is_pinned::<JournalLineRequest>(concat!(
        r#"{"account_id":"22222222-2222-4222-8222-222222222222","debit_minor":4500,"#,
        r#""credit_minor":0,"memo":null}"#,
    ));
    assert_json_is_pinned::<PostJournalRequest>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","entry_date":"2026-08-10","#,
        r#""description":"Groceries","reference":null,"lines":[{"account_id":"#,
        r#""22222222-2222-4222-8222-222222222222","debit_minor":4500,"credit_minor":0,"memo":"#,
        r#"null},{"account_id":"66666666-6666-4666-8666-666666666666","debit_minor":0,"#,
        r#""credit_minor":4500,"memo":"card"}]}"#,
    ));
}

#[test]
fn post_simple_entry_request_of_each_kind() {
    // An expense: category and wallet.
    assert_json_is_pinned::<PostSimpleEntryRequest>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"expense","#,
        r#""bill_status":null,"entry_date":"2026-08-10","description":"Groceries","#,
        r#""reference":null,"amount_minor":4500,"category_account_id":"#,
        r#""22222222-2222-4222-8222-222222222222","wallet_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","payable_account_id":null,"#,
        r#""from_account_id":null,"to_account_id":null}"#,
    ));
    // An unpaid bill: category and payable, with a status.
    assert_json_is_pinned::<PostSimpleEntryRequest>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"bill","bill_status":"#,
        r#""unpaid","entry_date":"2026-08-10","description":"Power","reference":"B-1","#,
        r#""amount_minor":9900,"category_account_id":"22222222-2222-4222-8222-222222222222","#,
        r#""wallet_account_id":null,"payable_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","from_account_id":null,"to_account_id":"#,
        r#"null}"#,
    ));
    // Paying an existing bill.
    assert_json_is_pinned::<PostSimpleEntryRequest>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"bill","bill_status":"#,
        r#""pay_existing","entry_date":"2026-08-10","description":"Power","reference":null,"#,
        r#""amount_minor":9900,"category_account_id":null,"wallet_account_id":"#,
        r#""22222222-2222-4222-8222-222222222222","payable_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","from_account_id":null,"to_account_id":"#,
        r#"null}"#,
    ));
    // A transfer: from and to.
    assert_json_is_pinned::<PostSimpleEntryRequest>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"transfer","#,
        r#""bill_status":null,"entry_date":"2026-08-10","description":"","reference":null,"#,
        r#""amount_minor":1,"category_account_id":null,"wallet_account_id":null,"#,
        r#""payable_account_id":null,"from_account_id":"#,
        r#""22222222-2222-4222-8222-222222222222","to_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666"}"#,
    ));
}

#[test]
fn a_request_with_a_missing_role_or_a_bad_date_still_reads() {
    // The form sends what the user has filled in so far. What is missing or
    // malformed is refused by core with its own code (`account_required`,
    // `bill_status_required`, `invalid_date`), not by the JSON layer, whose
    // failure the UI can only show as an unknown error.
    assert_json_is_pinned::<PostSimpleEntryRequest>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"bill","bill_status":"#,
        r#"null,"entry_date":"10/08/2026","description":"","reference":null,"amount_minor":0,"#,
        r#""category_account_id":null,"wallet_account_id":null,"payable_account_id":null,"#,
        r#""from_account_id":null,"to_account_id":null}"#,
    ));
    assert_json_is_pinned::<PostJournalRequest>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","entry_date":"not a date","#,
        r#""description":"","reference":null,"lines":[{"account_id":"#,
        r#""22222222-2222-4222-8222-222222222222","debit_minor":5,"credit_minor":5,"memo":"#,
        r#"null}]}"#,
    ));
}

#[test]
fn recurring_template_requests() {
    assert_json_is_pinned::<CreateRecurringTemplateRequest>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","name":"Rent","kind":"#,
        r#""expense","bill_status":null,"amount_minor":80000,"cadence":"monthly","#,
        r#""day_of_month":1,"category_account_id":"22222222-2222-4222-8222-222222222222","#,
        r#""wallet_account_id":"66666666-6666-4666-8666-666666666666","payable_account_id":"#,
        r#"null,"from_account_id":null,"to_account_id":null,"memo":null,"next_date":"#,
        r#""2026-09-01"}"#,
    ));
    assert_json_is_pinned::<UpdateRecurringTemplateRequest>(concat!(
        r#"{"id":"88888888-8888-4888-8888-888888888888","name":"Power","kind":"bill","#,
        r#""bill_status":"paid","amount_minor":9900,"cadence":"yearly","day_of_month":null,"#,
        r#""category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","payable_account_id":null,"#,
        r#""from_account_id":null,"to_account_id":null,"memo":"meter 7","next_date":"#,
        r#""2027-01-15"}"#,
    ));
}

#[test]
fn recurring_template_view_of_each_kind() {
    assert_json_is_pinned::<RecurringTemplateView>(concat!(
        r#"{"id":"88888888-8888-4888-8888-888888888888","entity_id":"#,
        r#""11111111-1111-4111-8111-111111111111","name":"Salary","kind":"income","#,
        r#""bill_status":null,"amount_minor":250000,"cadence":"monthly","day_of_month":28,"#,
        r#""category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","payable_account_id":null,"#,
        r#""from_account_id":null,"to_account_id":null,"memo":null,"next_date":"2026-08-28","#,
        r#""due":true}"#,
    ));
    assert_json_is_pinned::<RecurringTemplateView>(concat!(
        r#"{"id":"88888888-8888-4888-8888-888888888888","entity_id":"#,
        r#""11111111-1111-4111-8111-111111111111","name":"Savings","kind":"transfer","#,
        r#""bill_status":null,"amount_minor":5000,"cadence":"weekly","day_of_month":null,"#,
        r#""category_account_id":null,"wallet_account_id":null,"payable_account_id":null,"#,
        r#""from_account_id":"22222222-2222-4222-8222-222222222222","to_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","memo":"pot","next_date":"2026-08-14","#,
        r#""due":false}"#,
    ));
}

#[test]
fn recurring_post_result() {
    assert_json_is_pinned::<RecurringPostResult>(concat!(
        r#"{"entry":{"entry":{"id":"33333333-3333-4333-8333-333333333333","entity_id":"#,
        r#""11111111-1111-4111-8111-111111111111","entry_date":"2026-08-10","description":"#,
        r#""Power","reference":null,"status":"posted","hidden":false},"lines":[],"is_voided":"#,
        r#"false},"template":{"id":"88888888-8888-4888-8888-888888888888","entity_id":"#,
        r#""11111111-1111-4111-8111-111111111111","name":"Power","kind":"bill","bill_status":"#,
        r#""unpaid","amount_minor":9900,"cadence":"monthly","day_of_month":10,"#,
        r#""category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"#,
        r#"null,"payable_account_id":"66666666-6666-4666-8666-666666666666","#,
        r#""from_account_id":null,"to_account_id":null,"memo":null,"next_date":"2026-09-10","#,
        r#""due":false}}"#,
    ));
}

#[test]
fn reports() {
    assert_json_is_pinned::<TrialBalance>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","as_of":"2026-12-31","lines":"#,
        r#"[{"code":"1000","name":"Cash","account_type":"asset","debit_minor":4500,"#,
        r#""credit_minor":0,"balance_minor":4500,"synthetic":null}],"total_debits":4500,"#,
        r#""total_credits":4500}"#,
    ));
    assert_json_is_pinned::<PnL>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","from":"2026-01-01","to":"#,
        r#""2026-12-31","income":[],"expenses":[],"total_income":0,"total_expenses":0,"#,
        r#""net_income":0}"#,
    ));
    assert_json_is_pinned::<BalanceSheet>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","as_of":"2026-12-31","assets":"#,
        r#"{"lines":[],"total":0},"liabilities":{"lines":[],"total":0},"equity":{"lines":[{"#,
        r#""code":"","name":"","account_type":"equity","debit_minor":0,"credit_minor":7,"#,
        r#""balance_minor":7,"synthetic":"net_income"}],"total":7},"total_assets":0,"#,
        r#""total_liabilities_equity":7}"#,
    ));
}

#[test]
fn dashboard_summary() {
    assert_json_is_pinned::<DashboardSummary>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","base_currency":"EUR","#,
        r#""cash_like_assets":100,"income":50,"expenses":20,"net_income":30,"#,
        r#""recent_entry_count":3,"savings_rate_bps":6000,"spend_ratio_bps":4000,"#,
        r#""top_expense":{"code":"6100","name":"Rent","amount_minor":20,"share_bps":10000},"#,
        r#""net_vs_previous_bps":null}"#,
    ));
}

#[test]
fn cash_flow_series() {
    assert_json_is_pinned::<CashFlowSeries>(concat!(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","from":"2026-08-01","to":"#,
        r#""2026-08-31","granularity":"day","total_income_minor":50,"total_expenses_minor":"#,
        r#"20,"net_minor":30,"buckets":[{"start":"2026-08-01","end":"2026-08-01","#,
        r#""income_minor":50,"expenses_minor":20,"cumulative_income_minor":50,"#,
        r#""cumulative_expenses_minor":20}]}"#,
    ));
}

#[test]
fn csv_import_rows_carry_the_simple_entry_request() {
    assert_json_is_pinned::<CsvImportPostInput>(concat!(
        r#"{"rows":[{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"income","#,
        r#""bill_status":null,"entry_date":"2026-08-10","description":"Salary","reference":"#,
        r#"null,"amount_minor":250000,"category_account_id":"#,
        r#""22222222-2222-4222-8222-222222222222","wallet_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","payable_account_id":null,"#,
        r#""from_account_id":null,"to_account_id":null}],"include_duplicates":true}"#,
    ));
    // A suggestion is a part-filled form: the accounts are not chosen yet.
    assert_json_is_pinned::<CsvImportPreviewRow>(concat!(
        r#"{"source_row":2,"duplicate":false,"error":null,"suggested":{"entity_id":"#,
        r#""11111111-1111-4111-8111-111111111111","kind":"expense","bill_status":null,"#,
        r#""entry_date":"2026-08-10","description":"Cafe","reference":null,"amount_minor":"#,
        r#"350,"category_account_id":null,"wallet_account_id":null,"payable_account_id":null,"#,
        r#""from_account_id":null,"to_account_id":null},"signed_amount_minor":-350}"#,
    ));
}

#[test]
fn csv_import_preview_and_result() {
    assert_json_is_pinned::<CsvImportPreview>(concat!(
        r#"{"source":"bank.csv","headers":["Date","Amount"],"detected_mapping":{"date":"Date","#,
        r#""description":null,"amount":"Amount","debit":null,"credit":null,"reference":null,"#,
        r#""direction":null},"rows":[]}"#,
    ));
    assert_json_is_pinned::<CsvImportPostResult>(r#"{"posted":[],"skipped_duplicate_count":2}"#);
}

#[test]
fn journal_csv_line() {
    assert_json_is_pinned::<JournalCsvLine>(concat!(
        r#"{"date":"2026-08-10","description":"Groceries","reference":null,"account_code":"#,
        r#""1000","account_name":"Cash","debit_minor":0,"credit_minor":4500,"status":"#,
        r#""posted"}"#,
    ));
}

#[test]
fn document_meta() {
    assert_json_is_pinned::<DocumentMeta>(concat!(
        r#"{"id":"99999999-9999-4999-8999-999999999999","entity_id":"#,
        r#""11111111-1111-4111-8111-111111111111","entry_id":"#,
        r#""33333333-3333-4333-8333-333333333333","filename":"bill.pdf","mime_type":"#,
        r#""application/pdf","size_bytes":1024,"created_at":"unix:1791244800","#,
        r#""entry_description":"Power"}"#,
    ));
}

#[test]
fn document_suggestion() {
    // A reading with every field found, and its coded notes.
    assert_json_is_pinned::<DocumentSuggestion>(concat!(
        r#"{"source":"heuristic","model":"invoice-parser-v1","kind":"bill","amount_minor":7253,"#,
        r#""entry_date":"2026-08-13","description":"Volton — Gas bill","reference":"#,
        r#""NGS000000001","merchant":"Volton","bill_unpaid":true,"category_account_id":"#,
        r#""22222222-2222-4222-8222-222222222222","wallet_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","payable_account_id":"#,
        r#""77777777-7777-4777-8777-777777777777","confidence":0.75,"notes":[{"code":"#,
        r#""parsed_from_document_text","params":{}},{"code":"dated_from_document","params":"#,
        r#"{"date":"2026-08-13"}}]}"#,
    ));
    // A file nothing was read from.
    assert_json_is_pinned::<DocumentSuggestion>(concat!(
        r#"{"source":"none","model":null,"kind":"expense","amount_minor":null,"#,
        r#""entry_date":null,"description":null,"reference":null,"merchant":null,"#,
        r#""bill_unpaid":false,"category_account_id":null,"wallet_account_id":null,"#,
        r#""payable_account_id":null,"confidence":0.0,"notes":[{"code":"no_text_extracted","#,
        r#""params":{}}]}"#,
    ));
    // Text read by OCR, on an income document.
    assert_json_is_pinned::<DocumentSuggestion>(concat!(
        r#"{"source":"bundled_ocr","model":"ocrs-bundled","kind":"income","amount_minor":"#,
        r#"186000,"entry_date":null,"description":null,"reference":null,"merchant":null,"#,
        r#""bill_unpaid":false,"category_account_id":null,"wallet_account_id":null,"#,
        r#""payable_account_id":null,"confidence":0.5,"notes":[]}"#,
    ));
}

#[test]
fn a_suggestion_dated_on_a_day_the_calendar_lacks_is_refused_by_the_json_layer() {
    assert!(
        is_refused::<DocumentSuggestion>(concat!(
            r#"{"source":"none","model":null,"kind":"expense","amount_minor":null,"#,
            r#""entry_date":"2026-02-31","description":null,"reference":null,"merchant":null,"#,
            r#""bill_unpaid":false,"category_account_id":null,"wallet_account_id":null,"#,
            r#""payable_account_id":null,"confidence":0.0,"notes":[]}"#,
        )),
        "accepted"
    );
}

#[test]
fn analyzer_status() {
    assert_json_is_pinned::<AnalyzerStatus>(
        r#"{"ocr_available":true,"offline":true,"hint":"ready"}"#,
    );
    assert_json_is_pinned::<AnalyzerStatus>(
        r#"{"ocr_available":false,"offline":true,"hint":"models_missing"}"#,
    );
}

#[test]
fn an_analyzer_status_whose_fields_disagree_is_refused_by_the_json_layer() {
    for contradiction in [
        r#"{"ocr_available":true,"offline":true,"hint":"models_missing"}"#,
        r#"{"ocr_available":false,"offline":true,"hint":"ready"}"#,
        r#"{"ocr_available":true,"offline":false,"hint":"ready"}"#,
    ] {
        assert!(
            is_refused::<AnalyzerStatus>(contradiction),
            "{contradiction}"
        );
    }
}

#[test]
fn ui_prefs() {
    assert_json_is_pinned::<UiPrefs>(concat!(
        r#"{"locale":"el","last_entity_id":"11111111-1111-4111-8111-111111111111","#,
        r#""last_accounts_by_entity_kind":{"11111111-1111-4111-8111-111111111111:expense":{"#,
        r#""category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","payable_account_id":null,"#,
        r#""from_account_id":null,"to_account_id":null}}}"#,
    ));
}

/// The view is only ever written, so it is pinned from the file it is
/// loaded from instead of being read back: the keys of the file, in the
/// file's order, then `unreadable`.
#[test]
fn ui_prefs_view() {
    let stored = concat!(
        r#"{"locale":"el","last_entity_id":"11111111-1111-4111-8111-111111111111","#,
        r#""last_accounts_by_entity_kind":{"11111111-1111-4111-8111-111111111111:expense":{"#,
        r#""category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"#,
        r#""66666666-6666-4666-8666-666666666666","payable_account_id":null,"#,
        r#""from_account_id":null,"to_account_id":null}}"#,
    );
    let dir = tempfile::tempdir().expect("a temporary directory");
    let written = |text: &str| {
        std::fs::write(ui_prefs_path(dir.path()), text).expect("write the preferences file");
        serde_json::to_string(&load_ui_prefs_view(dir.path())).expect("the view serializes")
    };

    assert_eq!(
        written(&format!("{stored}}}")),
        format!(r#"{stored},"unreadable":false}}"#)
    );
    assert_eq!(
        written("not json"),
        concat!(
            r#"{"locale":"en","last_entity_id":null,"last_accounts_by_entity_kind":{},"#,
            r#""unreadable":true}"#,
        )
    );
}

#[test]
fn an_id_that_is_not_a_uuid_is_refused_by_the_json_layer() {
    assert!(
        is_refused::<VoidResult>(r#"{"original_id":"nope","reverse_id":"nope"}"#),
        "accepted"
    );
}

#[test]
fn a_view_with_a_malformed_date_is_refused_by_the_json_layer() {
    assert!(
        is_refused::<RegisterLine>(concat!(
            r#"{"entry_id":"33333333-3333-4333-8333-333333333333","entry_date":"2026-8-1","#,
            r#""description":"","debit_minor":0,"credit_minor":1,"balance_minor":0,"hidden":"#,
            r#"false}"#,
        )),
        "accepted"
    );
}
