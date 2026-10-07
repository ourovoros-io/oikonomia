//! Report and dashboard commands, and the PDF export.
//!
//! The report commands require the unlocked vault and compute their figures
//! in core. The PDF is different: the webview lays it out and hands over the
//! finished bytes, and [`report_export_pdf`] only saves them where the user
//! chooses, so that command never opens the vault.

use crate::commands::support::{
    SaveTarget, decode_capped_base64, save_with_dialog, with_connection,
};
use crate::error::CommandResult;
use crate::state::AppState;
use oikonomia_core::domain::EntityId;
use oikonomia_core::ledger::{
    BalanceSheet, CashFlowSeries, DashboardSummary, PnL, TrialBalance, balance_sheet,
    cash_flow_series_for_window, dashboard_summary, profit_and_loss, profit_and_loss_export,
    trial_balance,
};
use oikonomia_core::util::{DateText, utc_today};
use tauri::State;

/// The largest decoded PDF [`report_export_pdf`] accepts.
const MAX_PDF_EXPORT_BYTES: usize = 32 * 1024 * 1024;

/// Returns an entity's trial balance as of a date.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `invalid_date` when `as_of` is not a date, `not_found` when the
/// entity does not exist, `money_overflow` when a total does not fit the
/// money type, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn report_trial_balance(
    state: State<'_, AppState>,
    entity_id: EntityId,
    as_of: DateText,
) -> CommandResult<TrialBalance> {
    with_connection(&state, move |conn| {
        trial_balance(conn, entity_id, as_of.parse()?)
    })
    .await
}

/// Returns an entity's profit and loss for the dates `from` through `to`.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `invalid_date` when a bound is not a date, `date_range_inverted`
/// when `from` is after `to`, `not_found` when the entity does not exist,
/// `money_overflow` when a total does not fit the money type, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn report_pnl(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: DateText,
    to: DateText,
) -> CommandResult<PnL> {
    with_connection(&state, move |conn| {
        profit_and_loss(conn, entity_id, from.parse()?, to.parse()?)
    })
    .await
}

/// Returns the profit and loss that goes to other people: [`report_pnl`]
/// with hidden entries left out.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns the errors of [`report_pnl`].
#[tauri::command]
pub(crate) async fn report_pnl_export(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: DateText,
    to: DateText,
) -> CommandResult<PnL> {
    with_connection(&state, move |conn| {
        profit_and_loss_export(conn, entity_id, from.parse()?, to.parse()?)
    })
    .await
}

/// Returns an entity's balance sheet as of a date.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `invalid_date` when `as_of` is not a date, `not_found` when the
/// entity does not exist, `money_overflow` when a total does not fit the
/// money type, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn report_balance_sheet(
    state: State<'_, AppState>,
    entity_id: EntityId,
    as_of: DateText,
) -> CommandResult<BalanceSheet> {
    with_connection(&state, move |conn| {
        balance_sheet(conn, entity_id, as_of.parse()?)
    })
    .await
}

/// Returns the dashboard figures of an entity: income and expenses for the
/// dates `from` through `to`, and assets as of `assets_as_of`.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the entity does not exist, `invalid_date` when a
/// date argument is not a date, `date_range_inverted` when `from` is after
/// `to`, `money_overflow` when a total does not fit the money type, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn dashboard_summary_cmd(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: DateText,
    to: DateText,
    assets_as_of: DateText,
) -> CommandResult<DashboardSummary> {
    with_connection(&state, move |conn| {
        dashboard_summary(
            conn,
            entity_id,
            from.parse()?,
            to.parse()?,
            assets_as_of.parse()?,
        )
    })
    .await
}

/// Returns income and expenses per day or per month, for the cash-flow chart.
///
/// Requires the unlocked vault. An absent bound resolves to the entity's
/// first or last active entry, which is how the transactions page asks with
/// no date filter; with both bounds given the range is the dashboard's
/// period. Core settles the window ([`cash_flow_series_for_window`]); the
/// command supplies only the current date in UTC, which stands in for a
/// bound of a book that has no active entry.
///
/// # Errors
///
/// Returns `not_found` when the entity does not exist, `invalid_date` when a
/// bound is not a date, `date_range_inverted` when `from` is after `to`,
/// `money_overflow` when a total does not fit the money type, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn cash_flow_series_cmd(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: Option<DateText>,
    to: Option<DateText>,
) -> CommandResult<CashFlowSeries> {
    with_connection(&state, move |conn| {
        let from = DateText::parse_optional(from.as_ref())?;
        let to = DateText::parse_optional(to.as_ref())?;

        cash_flow_series_for_window(conn, entity_id, from, to, utc_today())
    })
    .await
}

/// Saves PDF bytes the webview built to a path chosen in a native save
/// dialog. Returns the path written, or `None` if the user cancelled.
///
/// Does not need the vault, so it also works while the vault is locked. The
/// call counts as activity for the idle watchdog. A chosen name without a
/// `.pdf` extension gets one.
///
/// # Errors
///
/// Returns `file_data_invalid` when `bytes_base64` is not base64,
/// `file_too_large` (with the cap as `max_mb`) for more than
/// [`MAX_PDF_EXPORT_BYTES`], `save_location_invalid` when the dialog's answer
/// is not a path, `save_failed` when the file cannot be written, and
/// `task_failed` when the blocking task panics.
#[tauri::command]
pub(crate) async fn report_export_pdf(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    bytes_base64: String,
    suggested_name: Option<String>,
) -> CommandResult<Option<String>> {
    let data = decode_pdf_export_bytes(&bytes_base64)?;
    state.touch();

    let target = SaveTarget {
        filter: Some(("PDF", &["pdf"])),
        file_name: pdf_export_file_name(suggested_name.as_deref()),
        complete_path: ensure_pdf_path,
    };
    save_with_dialog(&app, target, data).await
}

/// Decodes the PDF the webview sent, up to [`MAX_PDF_EXPORT_BYTES`].
///
/// # Errors
///
/// Returns `file_too_large` for a payload over the cap and
/// `file_data_invalid` for one that is not base64.
fn decode_pdf_export_bytes(bytes_base64: &str) -> CommandResult<Vec<u8>> {
    decode_capped_base64(bytes_base64, MAX_PDF_EXPORT_BYTES)
}

/// Returns the file name the save dialog suggests: the webview's suggestion,
/// trimmed, or `oikonomia-expenses.pdf` when it is absent or blank.
fn pdf_export_file_name(suggested_name: Option<&str>) -> String {
    match suggested_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => name.to_owned(),
        None => "oikonomia-expenses.pdf".to_owned(),
    }
}

/// Returns `path` with `.pdf` appended, unless it already has that extension
/// in any letter case.
///
/// A path with no file name becomes `oikonomia-expenses.pdf`.
fn ensure_pdf_path(path: std::path::PathBuf) -> std::path::PathBuf {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("pdf") => path,
        _ => {
            let mut name = path.file_name().map_or_else(
                || std::ffi::OsString::from("oikonomia-expenses"),
                std::ffi::OsString::from,
            );
            name.push(".pdf");
            match path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                Some(parent) => parent.join(name),
                None => std::path::PathBuf::from(name),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn decode_pdf_export_rejects_invalid_base64() {
        let err = decode_pdf_export_bytes("not-valid-base64!!!").expect_err("invalid");
        assert_eq!(err.code, "file_data_invalid");
    }

    #[test]
    fn decode_pdf_export_round_trips_tiny_pdf_header() {
        let pdf = b"%PDF-1.4\n";
        let encoded = base64::engine::general_purpose::STANDARD.encode(pdf);
        let decoded = decode_pdf_export_bytes(&encoded).expect("decode");
        assert_eq!(decoded, pdf);
        let padded = format!("  {encoded}  ");
        assert_eq!(decode_pdf_export_bytes(&padded).expect("trim"), pdf);
    }

    #[test]
    fn pdf_export_file_name_uses_trimmed_suggestion() {
        assert_eq!(
            pdf_export_file_name(Some("  august-2026.pdf  ")),
            "august-2026.pdf"
        );
        assert_eq!(pdf_export_file_name(Some("   ")), "oikonomia-expenses.pdf");
        assert_eq!(pdf_export_file_name(None), "oikonomia-expenses.pdf");
    }

    #[test]
    fn ensure_pdf_path_appends_extension() {
        let with = std::path::PathBuf::from("/tmp/report.PDF");
        assert_eq!(ensure_pdf_path(with.clone()), with);
        assert_eq!(
            ensure_pdf_path(std::path::PathBuf::from("/tmp/report")),
            std::path::PathBuf::from("/tmp/report.pdf")
        );
    }
}

/// `cash_flow_series_cmd` invoked through the mock IPC, the way the webview
/// invokes it.
///
/// Not built on Windows, where the mock runtime keeps a test executable from
/// starting; `commands::support::ipc_test_support` says why.
#[cfg(test)]
#[cfg(not(windows))]
mod ipc_tests {
    use crate::commands::reports::cash_flow_series_cmd;
    use crate::commands::support::ipc_test_support::MockApp;
    use oikonomia_core::domain::ChartTemplate;
    use oikonomia_core::ledger::{
        CreateEntity, PostSimpleEntry, PostSimpleEntryRequest, SimpleEntryKind, create_entity,
        list_accounts, post_simple_entry,
    };
    use oikonomia_core::prefs::Locale;
    use oikonomia_core::vault::Connection;

    /// Starts the mock app over a book that spent 25.00 on 5 August and
    /// earned 40.00 on 20 September 2026. Returns the book's id as the
    /// frontend holds it.
    fn mock_book(label: &str) -> (MockApp, String) {
        MockApp::start(
            label,
            tauri::generate_handler![cash_flow_series_cmd],
            seed_book,
        )
    }

    /// Creates the book and its two entries.
    fn seed_book(conn: &Connection) -> String {
        let book = CreateEntity {
            name: "Home".into(),
            base_currency: "EUR".into(),
            chart_template: ChartTemplate::Personal,
            fiscal_year_start_month: None,
        };
        let entity = create_entity(conn, &book, Locale::En).unwrap();
        let accounts = list_accounts(conn, entity.id).unwrap();
        let account = |code: &str| {
            accounts
                .iter()
                .find(|account| account.code == code)
                .map(|account| account.id)
                .unwrap()
        };

        for (kind, category, entry_date, amount_minor) in [
            (SimpleEntryKind::Expense, "5100", "2026-08-05", 2_500),
            (SimpleEntryKind::Income, "4000", "2026-09-20", 4_000),
        ] {
            let request = PostSimpleEntryRequest {
                entity_id: entity.id,
                kind,
                bill_status: None,
                entry_date: entry_date.into(),
                description: "seeded".into(),
                reference: None,
                amount_minor,
                category_account_id: Some(account(category)),
                wallet_account_id: Some(account("1010")),
                payable_account_id: None,
                from_account_id: None,
                to_account_id: None,
            };
            post_simple_entry(conn, &PostSimpleEntry::try_from(request).unwrap()).unwrap();
        }

        entity.id.to_string()
    }

    /// Invokes the command with the object `cashFlowSeries` in
    /// `web/src/lib/api.ts` passes to `invoke`: camelCase keys, and `null`
    /// for a bound the page leaves open.
    fn cash_flow(
        app: &MockApp,
        entity: &str,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<serde_json::Value, serde_json::Value> {
        app.invoke(
            "cash_flow_series_cmd",
            serde_json::json!({ "entityId": entity, "from": from, "to": to }),
        )
    }

    #[test]
    fn the_ipc_call_without_bounds_spans_the_first_to_the_last_active_entry() {
        let (app, entity) = mock_book("cash-flow-open");

        let series = cash_flow(&app, &entity, None, None).unwrap();

        assert_eq!(series["entity_id"], entity.as_str());
        assert_eq!(series["from"], "2026-08-05");
        assert_eq!(series["to"], "2026-09-20");
        assert_eq!(series["granularity"], "day");
        assert_eq!(series["total_income_minor"], 4_000);
        assert_eq!(series["total_expenses_minor"], 2_500);
        assert_eq!(series["net_minor"], 1_500);
        // 27 days of August and 20 of September.
        assert_eq!(series["buckets"].as_array().unwrap().len(), 47);
    }

    #[test]
    fn the_ipc_call_with_both_bounds_uses_them_as_given() {
        let (app, entity) = mock_book("cash-flow-bounded");

        let series = cash_flow(&app, &entity, Some("2026-08-01"), Some("2026-08-31")).unwrap();

        assert_eq!(series["from"], "2026-08-01");
        assert_eq!(series["to"], "2026-08-31");
        assert_eq!(series["total_income_minor"], 0);
        assert_eq!(series["total_expenses_minor"], 2_500);
        assert_eq!(series["buckets"].as_array().unwrap().len(), 31);
    }

    #[test]
    fn the_ipc_call_with_one_bound_fills_the_other_from_the_entries() {
        let (app, entity) = mock_book("cash-flow-half-open");

        let from_only = cash_flow(&app, &entity, Some("2026-09-01"), None).unwrap();
        let to_only = cash_flow(&app, &entity, None, Some("2026-08-31")).unwrap();

        assert_eq!(from_only["from"], "2026-09-01");
        assert_eq!(from_only["to"], "2026-09-20");
        assert_eq!(to_only["from"], "2026-08-05");
        assert_eq!(to_only["to"], "2026-08-31");
    }

    #[test]
    fn the_ipc_call_reports_a_refused_window_as_a_code() {
        let (app, entity) = mock_book("cash-flow-refused");

        let inverted =
            cash_flow(&app, &entity, Some("2026-09-01"), Some("2026-08-01")).unwrap_err();
        let malformed = cash_flow(&app, &entity, Some("1/8/2026"), None).unwrap_err();
        let unknown =
            cash_flow(&app, "99999999-9999-4999-8999-999999999999", None, None).unwrap_err();

        assert_eq!(inverted["code"], "date_range_inverted");
        assert_eq!(malformed["code"], "invalid_date");
        assert_eq!(unknown["code"], "not_found");
        assert_eq!(unknown["params"]["resource"], "entity");
    }
}
