//! Bank CSV import and journal CSV export commands.
//!
//! Import is two steps, so that nothing reaches the books unreviewed:
//! [`csv_import_preview`] parses a file into suggested rows and posts
//! nothing, and [`csv_import_post`] posts the rows the user kept. The file
//! comes from a native open dialog, or from a path that dialog returned
//! earlier and the webview passes back with a column mapping.
//!
//! A file whose date or amount column cannot be detected is not an error of
//! the first preview. It comes back with its headers and no rows
//! (`CsvImportPreview::missing_columns`), and the webview opens its Map
//! columns step on it, then asks again with the mapping the user chose.

use crate::commands::support::{
    FileDialog, SaveTarget, dialog_path, require_granted_path, run_blocking, save_with_dialog,
    with_connection,
};
use crate::error::CommandResult;
use crate::state::{AppState, GrantPurpose};
use oikonomia_core::csv::{
    CsvImportPostInput, CsvImportPostResult, CsvImportPreview, CsvImportPreviewInput,
    default_journal_export_file_name, ensure_csv_path, export_journal_csv, post_import_rows,
    preview_bank_csv_file,
};
use oikonomia_core::domain::EntityId;
use oikonomia_core::ledger::get_entity;
use std::path::PathBuf;
use tauri::{Runtime, State};

/// The file extensions the open dialog offers for a statement.
///
/// The importer reads comma-, semicolon- and tab-separated text
/// (`oikonomia_core::csv`), and banks export the last two as `.tsv` and
/// `.txt` as well as `.csv`. The filter only decides what the dialog shows;
/// the importer goes by the content, not the name.
const STATEMENT_EXTENSIONS: &[&str] = &["csv", "tsv", "txt"];

/// Parses a bank CSV file into suggested simple-entry rows. Posts nothing.
///
/// Requires the unlocked vault. When `input.path` is absent, a native open
/// dialog chooses the file; a given path must be one that dialog returned
/// earlier ([`GrantPurpose::Csv`]), so a file the user dropped on a window or
/// picked as a backup is refused. `input.mapping`, when set, replaces the
/// detection of columns from the header. A row that cannot be read is
/// reported in the preview, not as an error, and so is a file whose date or
/// amount column was not detected: its preview has no rows and names the
/// columns in `missing_columns`. Returns `None` if the user cancelled the
/// dialog.
///
/// # Errors
///
/// Returns `path_not_granted` for a path the user did not pick in the CSV
/// dialog;
/// `open_location_invalid` when the dialog's answer is not a path; `io` when
/// the file cannot be read; one of the `csv_` codes when it is over the size
/// limit, not UTF-8, empty, malformed or without a header row, or when the
/// column mapping is refused; `not_found` when
/// the entity or a role's account does not exist; `account_wrong_entity`
/// when an account belongs to another entity; and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn csv_import_preview<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    input: CsvImportPreviewInput,
) -> CommandResult<Option<CsvImportPreview>> {
    let path = if let Some(chosen) = input.path.clone() {
        let grants = state.path_grants();
        run_blocking(move || require_granted_path(&grants, GrantPurpose::Csv, &chosen)).await?
    } else {
        let Some(picked) = pick_csv_path(&app, &state).await? else {
            return Ok(None);
        };
        picked
    };

    with_connection(&state, move |conn| {
        preview_bank_csv_file(
            conn,
            input.entity_id,
            input.accounts(),
            &path,
            input.mapping.as_ref(),
        )
    })
    .await
    .map(Some)
}

/// Posts the preview rows the user kept, all in one transaction.
///
/// Requires the unlocked vault. A row that duplicates a posted entry (same
/// date, amount and normalized description) is skipped unless
/// `input.include_duplicates` is set. If any row is refused, no row is
/// posted.
///
/// # Errors
///
/// Returns `validation_internal` when the rows belong to more than one
/// entity, `not_found` when that entity does not exist, the
/// [simple-entry errors](crate::commands::journal#simple-entry-errors) for a
/// row that cannot be posted, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn csv_import_post(
    state: State<'_, AppState>,
    input: CsvImportPostInput,
) -> CommandResult<CsvImportPostResult> {
    with_connection(&state, move |conn| {
        post_import_rows(conn, &input.rows, input.include_duplicates)
    })
    .await
}

/// Exports an entity's journal as CSV to a path chosen in a native save
/// dialog.
///
/// Requires the unlocked vault. Hidden entries are left out. The file is
/// plaintext, written only where the user chose. Returns the path written, or
/// `None` if the user cancelled.
///
/// # Errors
///
/// Returns `not_found` when the entity does not exist, `serialization` when
/// the CSV cannot be produced, `save_location_invalid` when the dialog's answer
/// is not a path, `save_failed` when the file cannot be written, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn csv_export_journal(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Option<String>> {
    let (csv_text, file_name) = with_connection(&state, move |conn| {
        let entity = get_entity(conn, entity_id)?;
        let text = export_journal_csv(conn, entity_id)?;
        Ok((text, default_journal_export_file_name(&entity.name)))
    })
    .await?;

    let target = SaveTarget {
        filter: Some(("CSV", &["csv"])),
        file_name,
        complete_path: ensure_csv_path,
    };
    save_with_dialog(&app, target, csv_text.into_bytes()).await
}

/// Asks for a statement file ([`STATEMENT_EXTENSIONS`]) with a native open dialog.
///
/// The chosen path is granted for a CSV import and nothing else, so that a
/// second preview with a column mapping may pass it back. Returns `None` if
/// the user cancelled.
///
/// # Errors
///
/// Returns `open_location_invalid` when the dialog's answer is not a path,
/// and `task_failed` when the blocking task panics.
async fn pick_csv_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
) -> CommandResult<Option<PathBuf>> {
    let app = app.clone();
    let grants = state.path_grants();

    run_blocking(move || {
        use tauri_plugin_dialog::DialogExt;

        let dialog = app.dialog().file().add_filter("CSV", STATEMENT_EXTENSIONS);
        let Some(picked) = dialog.blocking_pick_file() else {
            return Ok(None);
        };

        let path = dialog_path(picked, FileDialog::OpenCsv)?;
        grants.grant(GrantPurpose::Csv, [path.clone()]);
        Ok(Some(path))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::STATEMENT_EXTENSIONS;

    #[test]
    fn the_open_dialog_offers_the_extensions_of_every_delimiter_the_importer_reads() {
        assert_eq!(STATEMENT_EXTENSIONS, ["csv", "tsv", "txt"]);
    }
}

/// `csv_import_preview` invoked through the mock IPC with the payloads the
/// webview sends: the first preview of a file, then the one that carries the
/// mapping from the Map columns step.
///
/// The path is granted by hand, where the app has the native dialog grant
/// it.
///
/// Not built on Windows, where the mock runtime keeps a test executable from
/// starting; `commands::support::ipc_test_support` says why.
#[cfg(test)]
#[cfg(not(windows))]
mod ipc_tests {
    use crate::commands::csv::csv_import_preview;
    use crate::commands::support::ipc_test_support::MockApp;
    use crate::state::GrantPurpose;
    use oikonomia_core::domain::ChartTemplate;
    use oikonomia_core::ledger::{CreateEntity, create_entity};
    use oikonomia_core::prefs::Locale;

    /// A statement whose date and amount headers detection does not know.
    const UNDETECTED_CSV: &[u8] = b"When,Memo,Paid\n2026-03-15,Rent,-800.00\n";

    /// Starts the mock app over a vault with one book in euros, with the
    /// preview command registered. Returns the book's id as the webview
    /// holds it.
    fn mock_book(label: &str) -> (MockApp, String) {
        MockApp::start(
            label,
            tauri::generate_handler![csv_import_preview],
            |conn| {
                let book = CreateEntity {
                    name: "Home".into(),
                    base_currency: "EUR".into(),
                    chart_template: ChartTemplate::Personal,
                    fiscal_year_start_month: None,
                };
                create_entity(conn, &book, Locale::En)
                    .unwrap()
                    .id
                    .to_string()
            },
        )
    }

    /// Writes `bytes` as a statement the CSV dialog returned, and gives its
    /// path.
    fn picked_statement(app: &MockApp, bytes: &[u8]) -> String {
        let path = app.write_file("bank.csv", bytes);
        app.grant(GrantPurpose::Csv, &path);
        path
    }

    /// The payload `csvImportPreview` in `web/src/lib/api.ts` builds: the
    /// three accounts always, the mapping only when there is one.
    fn payload(entity: &str, path: &str, mapping: Option<serde_json::Value>) -> serde_json::Value {
        let mut input = serde_json::json!({
            "entity_id": entity,
            "path": path,
            "wallet_account_id": null,
            "expense_account_id": null,
            "income_account_id": null,
        });
        if let Some(mapping) = mapping {
            input["mapping"] = mapping;
        }
        serde_json::json!({ "input": input })
    }

    #[test]
    fn a_file_whose_columns_are_not_detected_comes_back_for_mapping_not_as_an_error() {
        let (app, entity) = mock_book("csv-needs-mapping");
        let path = picked_statement(&app, UNDETECTED_CSV);

        let mut preview = app
            .invoke("csv_import_preview", payload(&entity, &path, None))
            .unwrap();

        // The source is the granted path with its links resolved, which a
        // temporary directory may have; the next test passes it back.
        let source = preview["source"].take();
        assert!(
            source
                .as_str()
                .is_some_and(|source| source.ends_with("bank.csv"))
        );
        assert_eq!(
            preview,
            serde_json::json!({
                "source": null,
                "headers": ["When", "Memo", "Paid"],
                "detected_mapping": {
                    "date": null,
                    "description": "Memo",
                    "amount": null,
                    "debit": null,
                    "credit": null,
                    "reference": null,
                    "direction": null,
                },
                "missing_columns": ["date", "amount"],
                "rows": [],
            })
        );
    }

    #[test]
    fn the_mapping_the_map_columns_step_sends_previews_that_file() {
        let (app, entity) = mock_book("csv-mapped");
        let path = picked_statement(&app, UNDETECTED_CSV);
        // The webview passes back the source of the first preview, not the
        // path it has never seen.
        let first = app
            .invoke("csv_import_preview", payload(&entity, &path, None))
            .unwrap();
        let path = first["source"].as_str().unwrap();
        // What `draftToMapping` in `web/src/lib/csvImport.ts` writes: every
        // key, `null` for a column that is not mapped.
        let mapping = serde_json::json!({
            "date": "When",
            "description": "Memo",
            "amount": "Paid",
            "debit": null,
            "credit": null,
            "reference": null,
            "direction": null,
        });

        let preview = app
            .invoke("csv_import_preview", payload(&entity, path, Some(mapping)))
            .unwrap();

        assert_eq!(preview["missing_columns"], serde_json::json!([]));
        assert_eq!(preview["rows"].as_array().map(Vec::len), Some(1));
        assert_eq!(preview["rows"][0]["error"], serde_json::Value::Null);
        assert_eq!(preview["rows"][0]["signed_amount_minor"], -80_000);
        assert_eq!(preview["rows"][0]["suggested"]["description"], "Rent");
    }

    #[test]
    fn a_mapping_with_no_description_column_previews_rows_without_one() {
        let (app, entity) = mock_book("csv-no-description");
        let path = picked_statement(&app, UNDETECTED_CSV);
        // The Map columns step with Description set to "Not mapped".
        let mapping = serde_json::json!({
            "date": "When",
            "description": null,
            "amount": "Paid",
            "debit": null,
            "credit": null,
            "reference": null,
            "direction": null,
        });

        let preview = app
            .invoke("csv_import_preview", payload(&entity, &path, Some(mapping)))
            .unwrap();

        assert_eq!(preview["rows"][0]["error"], serde_json::Value::Null);
        assert_eq!(preview["rows"][0]["suggested"]["description"], "");
        assert_eq!(preview["rows"][0]["signed_amount_minor"], -80_000);
    }

    #[test]
    fn an_incomplete_mapping_is_still_refused_with_its_problem() {
        let (app, entity) = mock_book("csv-incomplete-mapping");
        let path = picked_statement(&app, UNDETECTED_CSV);
        let mapping = serde_json::json!({ "date": "When", "description": "Memo" });

        let refusal = app
            .invoke("csv_import_preview", payload(&entity, &path, Some(mapping)))
            .unwrap_err();

        assert_eq!(refusal["code"], "csv_invalid_mapping");
        assert_eq!(refusal["params"]["problem"], "missing_amount");
    }

    #[test]
    fn a_type_column_of_transaction_kinds_is_not_sent_as_the_direction() {
        let (app, entity) = mock_book("csv-type-kinds");
        let path = picked_statement(&app, b"Date,Payee,Amount,Type\n2026-03-15,Shop,-8.00,POS\n");

        let preview = app
            .invoke("csv_import_preview", payload(&entity, &path, None))
            .unwrap();

        assert_eq!(
            preview["detected_mapping"]["direction"],
            serde_json::Value::Null
        );
        assert_eq!(preview["rows"][0]["error"], serde_json::Value::Null);
        assert_eq!(preview["rows"][0]["signed_amount_minor"], -800);
    }
}
