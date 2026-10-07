//! Bank CSV import and journal CSV export commands.
//!
//! Import is two steps, so that nothing reaches the books unreviewed:
//! [`csv_import_preview`] parses a file into suggested rows and posts
//! nothing, and [`csv_import_post`] posts the rows the user kept. The file
//! comes from a native open dialog, or from a path that dialog returned
//! earlier and the webview passes back with a column mapping.

use crate::commands::support::{
    SaveTarget, dialog_path, require_granted_path, run_blocking, save_with_dialog, with_connection,
};
use crate::error::CommandResult;
use crate::state::AppState;
use oikonomia_core::csv::{
    CsvImportPostInput, CsvImportPostResult, CsvImportPreview, CsvImportPreviewInput,
    default_journal_export_file_name, ensure_csv_path, export_journal_csv, post_import_rows,
    preview_bank_csv_file,
};
use oikonomia_core::domain::EntityId;
use oikonomia_core::ledger::get_entity;
use std::path::PathBuf;
use tauri::State;

/// Parses a bank CSV file into suggested simple-entry rows. Posts nothing.
///
/// Requires the unlocked vault. When `input.path` is absent, a native open
/// dialog chooses the file; a given path must be a granted one.
/// `input.mapping`, when set, replaces the detection of columns from the
/// header. A row that cannot be read is reported in the preview, not as an
/// error. Returns `None` if the user cancelled the dialog.
///
/// # Errors
///
/// Returns `path_not_granted` for a path the user never handed over;
/// `save_location_invalid` when the dialog's answer is not a path; `io` when
/// the file cannot be read; `csv_parse` when it is over the size limit, not
/// UTF-8, empty, or has no usable date and amount columns; `not_found` when
/// the entity or a role's account does not exist; `account_wrong_entity`
/// when an account belongs to another entity; and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn csv_import_preview(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: CsvImportPreviewInput,
) -> CommandResult<Option<CsvImportPreview>> {
    let path = if let Some(chosen) = input.path.clone() {
        let grants = state.path_grants();
        run_blocking(move || require_granted_path(&grants, &chosen)).await?
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
/// Returns `not_found` when the entity does not exist, `csv_parse` when the
/// CSV cannot be produced, `save_location_invalid` when the dialog's answer
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

/// Asks for a `.csv` file with a native open dialog.
///
/// The chosen path is granted, so that a second preview with a column
/// mapping may pass it back. Returns `None` if the user cancelled.
///
/// # Errors
///
/// Returns `save_location_invalid` when the dialog's answer is not a path,
/// and `task_failed` when the blocking task panics.
async fn pick_csv_path(app: &tauri::AppHandle, state: &AppState) -> CommandResult<Option<PathBuf>> {
    let app = app.clone();
    let grants = state.path_grants();

    run_blocking(move || {
        use tauri_plugin_dialog::DialogExt;

        let dialog = app.dialog().file().add_filter("CSV", &["csv"]);
        let Some(picked) = dialog.blocking_pick_file() else {
            return Ok(None);
        };

        let path = dialog_path(picked, "CSV")?;
        grants.grant([path.clone()]);
        Ok(Some(path))
    })
    .await
}
