//! Document analysis and storage commands.

use crate::commands::support::{
    SaveTarget, decode_document_base64, dropped_file_name, require_granted_path, run_blocking,
    save_with_dialog, stored_text_locale, with_connection,
};
use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::{AppState, GatedVault};
use base64::Engine;
use oikonomia_core::documents::{
    AnalyzeContext, AnalyzerStatus, DocumentId, DocumentMeta, DocumentSuggestion,
    analyze_document_bytes, analyzer_status, attach_document, delete_document, get_document,
    list_documents, suggest_accounts_for_entity,
};
use oikonomia_core::domain::{EntityId, JournalEntryId};
use oikonomia_core::ledger::get_entity;
use oikonomia_core::prefs::Locale;
use serde::Serialize;
use std::path::Path;
use tauri::State;

/// Whether the shipped on-device OCR models are available.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri hands a command its arguments by value"
)]
pub(crate) fn document_analyzer_status(state: State<'_, AppState>) -> AnalyzerStatus {
    analyzer_status(Some(state.ocr_model_dir().as_path()))
}

/// Analyze a picked file and return a draft entry suggestion.
///
/// Nothing is stored: the file reaches the vault only when the entry is
/// posted ([`entry_post_simple_with_document`](crate::commands::entry_post_simple_with_document)). Analysis is fully offline
/// (bundled OCR + heuristics). Nothing is sent to the network.
#[tauri::command]
pub(crate) async fn document_analyze(
    state: State<'_, AppState>,
    entity_id: EntityId,
    filename: String,
    mime_type: String,
    data_base64: String,
) -> CommandResult<DocumentSuggestion> {
    let data = decode_document_base64(&data_base64)?;

    let vault = state.vault();
    let model_dir = state.ocr_model_dir().clone();
    let data_dir = state.data_dir().to_path_buf();
    state.touch();

    run_blocking(move || {
        let locale = stored_text_locale(&data_dir);

        analyze_readonly(
            &vault, &model_dir, entity_id, &filename, &mime_type, &data, locale,
        )
    })
    .await
}

/// Analyze a file from a filesystem path (Tauri native drag-and-drop).
#[tauri::command]
pub(crate) async fn document_analyze_path(
    state: State<'_, AppState>,
    entity_id: EntityId,
    path: String,
) -> CommandResult<DocumentSuggestion> {
    let filename = dropped_file_name(&path);
    let grants = state.path_grants();
    let vault = state.vault();
    let model_dir = state.ocr_model_dir().clone();
    let data_dir = state.data_dir().to_path_buf();
    state.touch();

    run_blocking(move || {
        let path = require_granted_path(&grants, &path)?;
        let locale = stored_text_locale(&data_dir);

        // Reject oversized/unsupported drops from metadata alone — a stray
        // 10 GB drop must not be read into memory before failing the size cap.
        let meta = std::fs::metadata(&path).map_err(|e| {
            CommandError::desktop(
                DesktopError::FileUnreadable,
                format!("could not read dropped file: {e}"),
            )
        })?;
        let mime = oikonomia_core::documents::resolve_mime("", &filename);
        oikonomia_core::documents::validate_document_file(&filename, &mime, meta.len())?;

        let data = std::fs::read(&path).map_err(|e| {
            CommandError::desktop(
                DesktopError::FileUnreadable,
                format!("could not read dropped file: {e}"),
            )
        })?;
        analyze_readonly(
            &vault, &model_dir, entity_id, &filename, &mime, &data, locale,
        )
    })
    .await
}

/// Analyze a document in memory and suggest a draft entry. Persists
/// nothing: the file is stored only when the entry is posted
/// (`entry_post_simple_with_document`), keeping the no-orphan invariant.
#[expect(
    clippy::too_many_arguments,
    reason = "the document's name, type and bytes are separate arguments; tracked for the API pass"
)]
fn analyze_readonly(
    vault: &GatedVault,
    model_dir: &Path,
    entity_id: EntityId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
    locale: Locale,
) -> CommandResult<DocumentSuggestion> {
    let mime = oikonomia_core::documents::resolve_mime(mime_type, filename);
    oikonomia_core::documents::validate_document_file(filename, &mime, data.len() as u64)?;

    let (accounts, entity) = {
        let guard = vault.acquire();
        let conn = guard.connection()?;
        (
            suggest_accounts_for_entity(conn, entity_id)?,
            get_entity(conn, entity_id)?,
        )
    };

    let suggestion = analyze_document_bytes(
        filename,
        &mime,
        data,
        &AnalyzeContext {
            template: entity.chart_template,
            accounts: &accounts,
            default_currency: &entity.base_currency,
            locale,
        },
        Some(model_dir),
    )?;

    Ok(suggestion)
}

/// Metadata plus base64 payload for the in-app viewer.
#[derive(Debug, Serialize)]
pub(crate) struct DocumentContent {
    /// Metadata.
    pub meta: DocumentMeta,
    /// Raw bytes, base64-encoded for IPC (bounded by the 8 MiB cap).
    pub data_base64: String,
}

/// All stored documents for an entity (metadata only).
#[tauri::command]
pub(crate) async fn document_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<DocumentMeta>> {
    with_connection(&state, move |conn| list_documents(conn, entity_id)).await
}

/// One document's bytes for the in-app viewer. Decrypted content crosses
/// IPC only; nothing is written to disk.
#[tauri::command]
pub(crate) async fn document_get(
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<DocumentContent> {
    with_connection(&state, move |conn| {
        let (meta, data) = get_document(conn, document_id)?;
        Ok(DocumentContent {
            meta,
            data_base64: base64::engine::general_purpose::STANDARD.encode(data),
        })
    })
    .await
}

/// Permanently delete a stored document.
#[tauri::command]
pub(crate) async fn document_delete(
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<()> {
    with_connection(&state, move |conn| delete_document(conn, document_id)).await
}

/// Attach a file to an existing posted entry. No OCR pass — analysis only
/// runs on the drop-zone flow; `analysis_json` stays NULL here.
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is one field of the IPC payload; tracked for the API pass"
)]
pub(crate) async fn document_attach(
    state: State<'_, AppState>,
    entity_id: EntityId,
    entry_id: JournalEntryId,
    filename: String,
    mime_type: String,
    data_base64: String,
) -> CommandResult<DocumentMeta> {
    let data = decode_document_base64(&data_base64)?;

    with_connection(&state, move |conn| {
        attach_document(conn, entity_id, entry_id, &filename, &mime_type, &data)
    })
    .await
}

/// Export a stored document, decrypted, to a path chosen in a native Save
/// dialog. Returns the path written, or `None` if the user cancelled.
///
/// Like the journal CSV export ([`csv_export_journal`](crate::commands::csv_export_journal)) and the report PDF
/// ([`report_export_pdf`](crate::commands::report_export_pdf)), this writes plaintext to disk, and like them only
/// to a path the user picked in the dialog ([`save_with_dialog`]).
#[tauri::command]
pub(crate) async fn document_export(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<Option<String>> {
    let (meta, data) = with_connection(&state, move |conn| get_document(conn, document_id)).await?;

    let target = SaveTarget {
        filter: None,
        file_name: meta.filename,
        complete_path: std::convert::identity,
    };
    save_with_dialog(&app, target, data).await
}
