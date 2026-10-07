//! The JSON form of every type that crosses IPC, pinned as literal text.
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

use oikonomia_core::csv::{CsvImportPostInput, CsvImportPreviewRow, JournalCsvLine};
use oikonomia_core::documents::DocumentMeta;
use oikonomia_core::domain::{Account, Entity, JournalEntry, JournalLine};
use oikonomia_core::ledger::{
    BalanceSheet, CashFlowSeries, CreateAccount, CreateEntity, CreateRecurringTemplateRequest,
    DashboardSummary, JournalLineRequest, PnL, PostJournalRequest, PostSimpleEntryRequest,
    PostedEntryView, RecurringPostResult, RecurringTemplateView, RegisterLine, TrialBalance,
    UpdateAccount, UpdateRecurringTemplateRequest, VoidResult,
};
use oikonomia_core::prefs::UiPrefs;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Reads `json` as a `T` and checks that writing the value gives `json` back.
#[track_caller]
fn assert_json_is_pinned<T: Serialize + DeserializeOwned>(json: &str) {
    let value: T = serde_json::from_str(json).expect("the pinned JSON reads as the type");
    let written = serde_json::to_string(&value).expect("the value serializes");

    assert_eq!(written, json);
}

/// Returns the error text of reading `json` as a `T`, which must fail.
#[track_caller]
fn rejection<T: DeserializeOwned>(json: &str) -> String {
    match serde_json::from_str::<T>(json) {
        Ok(_) => String::from("accepted"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn entity() {
    assert_json_is_pinned::<Entity>(
        r#"{"id":"11111111-1111-4111-8111-111111111111","name":"Home","base_currency":"EUR","fiscal_year_start_month":4,"chart_template":"personal"}"#,
    );
}

#[test]
fn create_entity() {
    assert_json_is_pinned::<CreateEntity>(
        r#"{"name":"Acme","base_currency":"USD","chart_template":"company","fiscal_year_start_month":null}"#,
    );
    assert_json_is_pinned::<CreateEntity>(
        r#"{"name":"Acme","base_currency":"USD","chart_template":"blank","fiscal_year_start_month":12}"#,
    );
}

#[test]
fn account() {
    assert_json_is_pinned::<Account>(
        r#"{"id":"22222222-2222-4222-8222-222222222222","entity_id":"11111111-1111-4111-8111-111111111111","code":"1000","name":"Cash","account_type":"asset","parent_id":null,"is_active":true,"is_system":false,"sort_order":10}"#,
    );
}

#[test]
fn create_and_update_account() {
    assert_json_is_pinned::<CreateAccount>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","code":"6100","name":"Rent","account_type":"expense","sort_order":null}"#,
    );
    assert_json_is_pinned::<UpdateAccount>(
        r#"{"id":"22222222-2222-4222-8222-222222222222","code":"6100","name":"Rent","is_active":false,"sort_order":3}"#,
    );
}

#[test]
fn journal_entry() {
    assert_json_is_pinned::<JournalEntry>(
        r#"{"id":"33333333-3333-4333-8333-333333333333","entity_id":"11111111-1111-4111-8111-111111111111","entry_date":"2026-08-10","description":"Groceries","reference":"INV-7","status":"posted","hidden":false}"#,
    );
}

#[test]
fn journal_line_on_each_side() {
    assert_json_is_pinned::<JournalLine>(
        r#"{"id":"44444444-4444-4444-8444-444444444444","entry_id":"33333333-3333-4333-8333-333333333333","account_id":"22222222-2222-4222-8222-222222222222","debit":{"amount_minor":4500},"credit":{"amount_minor":0},"memo":null}"#,
    );
    assert_json_is_pinned::<JournalLine>(
        r#"{"id":"44444444-4444-4444-8444-444444444444","entry_id":"33333333-3333-4333-8333-333333333333","account_id":"22222222-2222-4222-8222-222222222222","debit":{"amount_minor":0},"credit":{"amount_minor":4500},"memo":"card"}"#,
    );
}

#[test]
fn posted_entry_view() {
    assert_json_is_pinned::<PostedEntryView>(
        r#"{"entry":{"id":"33333333-3333-4333-8333-333333333333","entity_id":"11111111-1111-4111-8111-111111111111","entry_date":"2026-08-10","description":"Groceries","reference":null,"status":"posted","hidden":true},"lines":[{"id":"44444444-4444-4444-8444-444444444444","entry_id":"33333333-3333-4333-8333-333333333333","account_id":"22222222-2222-4222-8222-222222222222","debit":{"amount_minor":4500},"credit":{"amount_minor":0},"memo":null},{"id":"55555555-5555-4555-8555-555555555555","entry_id":"33333333-3333-4333-8333-333333333333","account_id":"66666666-6666-4666-8666-666666666666","debit":{"amount_minor":0},"credit":{"amount_minor":4500},"memo":null}],"is_voided":false}"#,
    );
}

#[test]
fn register_line() {
    assert_json_is_pinned::<RegisterLine>(
        r#"{"entry_id":"33333333-3333-4333-8333-333333333333","entry_date":"2026-08-10","description":"Groceries","debit_minor":0,"credit_minor":4500,"balance_minor":-4500,"hidden":false}"#,
    );
}

#[test]
fn void_result() {
    assert_json_is_pinned::<VoidResult>(
        r#"{"original_id":"33333333-3333-4333-8333-333333333333","reverse_id":"77777777-7777-4777-8777-777777777777"}"#,
    );
}

#[test]
fn post_journal_request() {
    assert_json_is_pinned::<JournalLineRequest>(
        r#"{"account_id":"22222222-2222-4222-8222-222222222222","debit_minor":4500,"credit_minor":0,"memo":null}"#,
    );
    assert_json_is_pinned::<PostJournalRequest>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","entry_date":"2026-08-10","description":"Groceries","reference":null,"lines":[{"account_id":"22222222-2222-4222-8222-222222222222","debit_minor":4500,"credit_minor":0,"memo":null},{"account_id":"66666666-6666-4666-8666-666666666666","debit_minor":0,"credit_minor":4500,"memo":"card"}]}"#,
    );
}

#[test]
fn post_simple_entry_request_of_each_kind() {
    // An expense: category and wallet.
    assert_json_is_pinned::<PostSimpleEntryRequest>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"expense","bill_status":null,"entry_date":"2026-08-10","description":"Groceries","reference":null,"amount_minor":4500,"category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"66666666-6666-4666-8666-666666666666","payable_account_id":null,"from_account_id":null,"to_account_id":null}"#,
    );
    // An unpaid bill: category and payable, with a status.
    assert_json_is_pinned::<PostSimpleEntryRequest>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"bill","bill_status":"unpaid","entry_date":"2026-08-10","description":"Power","reference":"B-1","amount_minor":9900,"category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":null,"payable_account_id":"66666666-6666-4666-8666-666666666666","from_account_id":null,"to_account_id":null}"#,
    );
    // Paying an existing bill.
    assert_json_is_pinned::<PostSimpleEntryRequest>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"bill","bill_status":"pay_existing","entry_date":"2026-08-10","description":"Power","reference":null,"amount_minor":9900,"category_account_id":null,"wallet_account_id":"22222222-2222-4222-8222-222222222222","payable_account_id":"66666666-6666-4666-8666-666666666666","from_account_id":null,"to_account_id":null}"#,
    );
    // A transfer: from and to.
    assert_json_is_pinned::<PostSimpleEntryRequest>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"transfer","bill_status":null,"entry_date":"2026-08-10","description":"","reference":null,"amount_minor":1,"category_account_id":null,"wallet_account_id":null,"payable_account_id":null,"from_account_id":"22222222-2222-4222-8222-222222222222","to_account_id":"66666666-6666-4666-8666-666666666666"}"#,
    );
}

#[test]
fn a_request_with_a_missing_role_or_a_bad_date_still_reads() {
    // The form sends what the user has filled in so far. What is missing or
    // malformed is refused by core with its own code (`account_required`,
    // `bill_status_required`, `invalid_date`), not by the JSON layer, whose
    // failure the UI can only show as an unknown error.
    assert_json_is_pinned::<PostSimpleEntryRequest>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"bill","bill_status":null,"entry_date":"10/08/2026","description":"","reference":null,"amount_minor":0,"category_account_id":null,"wallet_account_id":null,"payable_account_id":null,"from_account_id":null,"to_account_id":null}"#,
    );
    assert_json_is_pinned::<PostJournalRequest>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","entry_date":"not a date","description":"","reference":null,"lines":[{"account_id":"22222222-2222-4222-8222-222222222222","debit_minor":5,"credit_minor":5,"memo":null}]}"#,
    );
}

#[test]
fn recurring_template_requests() {
    assert_json_is_pinned::<CreateRecurringTemplateRequest>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","name":"Rent","kind":"expense","bill_status":null,"amount_minor":80000,"cadence":"monthly","day_of_month":1,"category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"66666666-6666-4666-8666-666666666666","payable_account_id":null,"from_account_id":null,"to_account_id":null,"memo":null,"next_date":"2026-09-01"}"#,
    );
    assert_json_is_pinned::<UpdateRecurringTemplateRequest>(
        r#"{"id":"88888888-8888-4888-8888-888888888888","name":"Power","kind":"bill","bill_status":"paid","amount_minor":9900,"cadence":"yearly","day_of_month":null,"category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"66666666-6666-4666-8666-666666666666","payable_account_id":null,"from_account_id":null,"to_account_id":null,"memo":"meter 7","next_date":"2027-01-15"}"#,
    );
}

#[test]
fn recurring_template_view_of_each_kind() {
    assert_json_is_pinned::<RecurringTemplateView>(
        r#"{"id":"88888888-8888-4888-8888-888888888888","entity_id":"11111111-1111-4111-8111-111111111111","name":"Salary","kind":"income","bill_status":null,"amount_minor":250000,"cadence":"monthly","day_of_month":28,"category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"66666666-6666-4666-8666-666666666666","payable_account_id":null,"from_account_id":null,"to_account_id":null,"memo":null,"next_date":"2026-08-28","due":true}"#,
    );
    assert_json_is_pinned::<RecurringTemplateView>(
        r#"{"id":"88888888-8888-4888-8888-888888888888","entity_id":"11111111-1111-4111-8111-111111111111","name":"Savings","kind":"transfer","bill_status":null,"amount_minor":5000,"cadence":"weekly","day_of_month":null,"category_account_id":null,"wallet_account_id":null,"payable_account_id":null,"from_account_id":"22222222-2222-4222-8222-222222222222","to_account_id":"66666666-6666-4666-8666-666666666666","memo":"pot","next_date":"2026-08-14","due":false}"#,
    );
}

#[test]
fn recurring_post_result() {
    assert_json_is_pinned::<RecurringPostResult>(
        r#"{"entry":{"entry":{"id":"33333333-3333-4333-8333-333333333333","entity_id":"11111111-1111-4111-8111-111111111111","entry_date":"2026-08-10","description":"Power","reference":null,"status":"posted","hidden":false},"lines":[],"is_voided":false},"template":{"id":"88888888-8888-4888-8888-888888888888","entity_id":"11111111-1111-4111-8111-111111111111","name":"Power","kind":"bill","bill_status":"unpaid","amount_minor":9900,"cadence":"monthly","day_of_month":10,"category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":null,"payable_account_id":"66666666-6666-4666-8666-666666666666","from_account_id":null,"to_account_id":null,"memo":null,"next_date":"2026-09-10","due":false}}"#,
    );
}

#[test]
fn reports() {
    assert_json_is_pinned::<TrialBalance>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","as_of":"2026-12-31","lines":[{"code":"1000","name":"Cash","account_type":"asset","debit_minor":4500,"credit_minor":0,"balance_minor":4500,"synthetic":null}],"total_debits":4500,"total_credits":4500}"#,
    );
    assert_json_is_pinned::<PnL>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","from":"2026-01-01","to":"2026-12-31","income":[],"expenses":[],"total_income":0,"total_expenses":0,"net_income":0}"#,
    );
    assert_json_is_pinned::<BalanceSheet>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","as_of":"2026-12-31","assets":{"lines":[],"total":0},"liabilities":{"lines":[],"total":0},"equity":{"lines":[{"code":"","name":"","account_type":"equity","debit_minor":0,"credit_minor":7,"balance_minor":7,"synthetic":"net_income"}],"total":7},"total_assets":0,"total_liabilities_equity":7}"#,
    );
}

#[test]
fn dashboard_summary() {
    assert_json_is_pinned::<DashboardSummary>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","base_currency":"EUR","cash_like_assets":100,"income":50,"expenses":20,"net_income":30,"recent_entry_count":3,"savings_rate_bps":6000,"spend_ratio_bps":4000,"top_expense":{"code":"6100","name":"Rent","amount_minor":20,"share_bps":10000},"net_vs_previous_bps":null}"#,
    );
}

#[test]
fn cash_flow_series() {
    assert_json_is_pinned::<CashFlowSeries>(
        r#"{"entity_id":"11111111-1111-4111-8111-111111111111","from":"2026-08-01","to":"2026-08-31","granularity":"day","total_income_minor":50,"total_expenses_minor":20,"net_minor":30,"buckets":[{"start":"2026-08-01","end":"2026-08-01","income_minor":50,"expenses_minor":20,"cumulative_income_minor":50,"cumulative_expenses_minor":20}]}"#,
    );
}

#[test]
fn csv_import_rows_carry_the_simple_entry_request() {
    assert_json_is_pinned::<CsvImportPostInput>(
        r#"{"rows":[{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"income","bill_status":null,"entry_date":"2026-08-10","description":"Salary","reference":null,"amount_minor":250000,"category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"66666666-6666-4666-8666-666666666666","payable_account_id":null,"from_account_id":null,"to_account_id":null}],"include_duplicates":true}"#,
    );
    // A suggestion is a part-filled form: the accounts are not chosen yet.
    assert_json_is_pinned::<CsvImportPreviewRow>(
        r#"{"source_row":2,"duplicate":false,"error":null,"suggested":{"entity_id":"11111111-1111-4111-8111-111111111111","kind":"expense","bill_status":null,"entry_date":"2026-08-10","description":"Cafe","reference":null,"amount_minor":350,"category_account_id":null,"wallet_account_id":null,"payable_account_id":null,"from_account_id":null,"to_account_id":null},"signed_amount_minor":-350}"#,
    );
}

#[test]
fn journal_csv_line() {
    assert_json_is_pinned::<JournalCsvLine>(
        r#"{"date":"2026-08-10","description":"Groceries","reference":null,"account_code":"1000","account_name":"Cash","debit_minor":0,"credit_minor":4500,"status":"posted"}"#,
    );
}

#[test]
fn document_meta() {
    assert_json_is_pinned::<DocumentMeta>(
        r#"{"id":"99999999-9999-4999-8999-999999999999","entity_id":"11111111-1111-4111-8111-111111111111","entry_id":"33333333-3333-4333-8333-333333333333","filename":"bill.pdf","mime_type":"application/pdf","size_bytes":1024,"created_at":"unix:1791244800","entry_description":"Power"}"#,
    );
}

#[test]
fn ui_prefs() {
    assert_json_is_pinned::<UiPrefs>(
        r#"{"locale":"el","last_entity_id":"11111111-1111-4111-8111-111111111111","last_accounts_by_entity_kind":{"11111111-1111-4111-8111-111111111111:expense":{"category_account_id":"22222222-2222-4222-8222-222222222222","wallet_account_id":"66666666-6666-4666-8666-666666666666","payable_account_id":null,"from_account_id":null,"to_account_id":null}}}"#,
    );
}

#[test]
fn an_id_that_is_not_a_uuid_is_refused_by_the_json_layer() {
    assert_ne!(
        rejection::<VoidResult>(r#"{"original_id":"nope","reverse_id":"nope"}"#),
        "accepted"
    );
}

#[test]
fn a_view_with_a_malformed_date_is_refused_by_the_json_layer() {
    assert_ne!(
        rejection::<RegisterLine>(
            r#"{"entry_id":"33333333-3333-4333-8333-333333333333","entry_date":"2026-8-1","description":"","debit_minor":0,"credit_minor":1,"balance_minor":0,"hidden":false}"#
        ),
        "accepted"
    );
}
