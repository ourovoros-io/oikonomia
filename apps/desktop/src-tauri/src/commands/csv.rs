//! Bank CSV import and journal CSV export commands.

use crate::commands::support::{
    SaveTarget, await_blocking, require_granted_path, save_with_dialog, with_vault_blocking,
};
use crate::error::{CommandError, CommandResult, DesktopError};
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

/// Parse a bank CSV into suggested simple-entry rows. **Does not post.**
///
/// When `input.path` is omitted, a native Open dialog chooses the file.
/// `input.mapping` overrides header auto-detect when set.
/// Returns `None` if the user cancelled the dialog.
#[tauri::command]
pub(crate) async fn csv_import_preview(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: CsvImportPreviewInput,
) -> CommandResult<Option<CsvImportPreview>> {
    let path = if let Some(chosen) = input.path.as_deref() {
        require_granted_path(&state, chosen)?
    } else {
        let Some(picked) = pick_csv_path(&app, &state).await? else {
            return Ok(None);
        };
        picked
    };

    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
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

/// Post selected preview rows through `post_simple_entry`.
///
/// Duplicates (date + amount + normalized description) are skipped unless
/// `include_duplicates` is true. Junk / unbalanced rows fail the whole batch.
#[tauri::command]
pub(crate) async fn csv_import_post(
    state: State<'_, AppState>,
    input: CsvImportPostInput,
) -> CommandResult<CsvImportPostResult> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        post_import_rows(conn, &input.rows, input.include_duplicates)
    })
    .await
}

/// Export the current entity's journal as CSV via a native Save dialog.
///
/// Returns the destination path, or `None` if the user cancelled.
#[tauri::command]
pub(crate) async fn csv_export_journal(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Option<String>> {
    let (csv_text, file_name) = with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
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

/// Native Open dialog for a `.csv` file. `None` if cancelled. The chosen path
/// is granted so a re-preview with a column mapping may pass it back.
async fn pick_csv_path(app: &tauri::AppHandle, state: &AppState) -> CommandResult<Option<PathBuf>> {
    let picked = await_blocking(tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            use tauri_plugin_dialog::DialogExt;
            Ok(app
                .dialog()
                .file()
                .add_filter("CSV", &["csv"])
                .blocking_pick_file())
        }
    }))
    .await?;
    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path.into_path().map_err(|e| {
        CommandError::desktop(
            DesktopError::SaveLocationInvalid,
            format!("invalid CSV location: {e}"),
        )
    })?;
    state.grant_paths([path.clone()]);
    Ok(Some(path))
}
