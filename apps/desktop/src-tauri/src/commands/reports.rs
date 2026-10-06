//! Report, dashboard and PDF export commands.

use crate::commands::support::{
    SaveTarget, decode_capped_base64, save_with_dialog, with_vault_blocking,
};
use crate::error::CommandResult;
use crate::state::AppState;
use oikonomia_core::domain::EntityId;
use oikonomia_core::ledger::{
    BalanceSheet, CashFlowSeries, DashboardSummary, PnL, TrialBalance, activity_window,
    balance_sheet, cash_flow_series, dashboard_summary, profit_and_loss, profit_and_loss_export,
    trial_balance,
};
use oikonomia_core::util::{format_date, utc_today};
use tauri::State;

/// Trial balance.
#[tauri::command]
pub(crate) async fn report_trial_balance(
    state: State<'_, AppState>,
    entity_id: EntityId,
    as_of: String,
) -> CommandResult<TrialBalance> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        trial_balance(conn, entity_id, &as_of)
    })
    .await
}

/// Profit and loss.
#[tauri::command]
pub(crate) async fn report_pnl(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: String,
    to: String,
) -> CommandResult<PnL> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        profit_and_loss(conn, entity_id, &from, &to)
    })
    .await
}

/// Accountant / PDF export P&L. Same args as [`report_pnl`]; Hidden omitted.
#[tauri::command]
pub(crate) async fn report_pnl_export(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: String,
    to: String,
) -> CommandResult<PnL> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        profit_and_loss_export(conn, entity_id, &from, &to)
    })
    .await
}

/// Balance sheet.
#[tauri::command]
pub(crate) async fn report_balance_sheet(
    state: State<'_, AppState>,
    entity_id: EntityId,
    as_of: String,
) -> CommandResult<BalanceSheet> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        balance_sheet(conn, entity_id, &as_of)
    })
    .await
}

/// Saves PDF bytes the webview built to a path chosen in a native Save
/// dialog. Returns the path written, or `None` if the user cancelled.
///
/// The vault is not opened, so this also works while it is locked. The call
/// counts as activity for the idle watchdog.
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

/// Decoded PDF cap for a monthly expense report (webview-generated).
const MAX_PDF_EXPORT_BYTES: usize = 32 * 1024 * 1024;

fn decode_pdf_export_bytes(bytes_base64: &str) -> CommandResult<Vec<u8>> {
    decode_capped_base64(bytes_base64, MAX_PDF_EXPORT_BYTES)
}

fn pdf_export_file_name(suggested_name: Option<&str>) -> String {
    match suggested_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => name.to_owned(),
        None => "oikonomia-expenses.pdf".to_owned(),
    }
}

fn ensure_pdf_path(path: std::path::PathBuf) -> std::path::PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("pdf") => path,
        _ => {
            let mut name = path.file_name().map_or_else(
                || std::ffi::OsString::from("oikonomia-expenses"),
                std::ffi::OsString::from,
            );
            name.push(".pdf");
            match path.parent().filter(|p| !p.as_os_str().is_empty()) {
                Some(parent) => parent.join(name),
                None => std::path::PathBuf::from(name),
            }
        }
    }
}

/// Dashboard summary.
#[tauri::command]
pub(crate) async fn dashboard_summary_cmd(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: String,
    to: String,
    assets_as_of: String,
) -> CommandResult<DashboardSummary> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        dashboard_summary(conn, entity_id, &from, &to, &assets_as_of)
    })
    .await
}

/// Income and expenses per day or month for the cash-flow light. An empty
/// bound resolves to the book's first or last active entry (Transactions with
/// no date filter); both bounds set is the dashboard's period.
#[tauri::command]
pub(crate) async fn cash_flow_series_cmd(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: Option<String>,
    to: Option<String>,
) -> CommandResult<CashFlowSeries> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        let (start, end) =
            activity_window(conn, entity_id, from.as_deref(), to.as_deref(), utc_today())?;
        cash_flow_series(conn, entity_id, &format_date(start), &format_date(end))
    })
    .await
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
