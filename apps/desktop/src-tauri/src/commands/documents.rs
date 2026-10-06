//! Document commands: analyze a bill or receipt into a draft entry, and
//! store, list, show, export and delete the documents attached to entries.
//!
//! Analysis is offline: the text is read by the OCR models shipped with the
//! app and nothing is sent anywhere. Analysis also stores nothing. A document
//! reaches the vault only together with the entry it belongs to
//! ([`entry_post_simple_with_document`](crate::commands::entry_post_simple_with_document)),
//! so a document without an entry cannot exist.
//!
//! A document arrives in one of two ways. Picked in the webview, it comes as
//! base64 with a name and a type. Dropped on a window, it comes as a path,
//! which must be a granted one and is read here.

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

/// Returns whether the OCR models shipped with the app were found.
///
/// Needs no vault.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri hands a command its arguments by value"
)]
pub(crate) fn document_analyzer_status(state: State<'_, AppState>) -> AnalyzerStatus {
    analyzer_status(Some(state.ocr_model_dir().as_path()))
}

/// Analyzes a document the webview picked and returns a draft entry
/// suggestion.
///
/// Requires the unlocked vault, for the entity's accounts and currency.
/// Nothing is stored. Core reads the document's text with the app's stored
/// language.
///
/// # Errors
///
/// Returns `file_data_invalid` when `data_base64` is not base64;
/// `file_too_large` (with the cap as `max_mb`), `file_empty`,
/// `file_type_unsupported` and `name_required` when the document is refused;
/// `not_found` when the entity does not exist; and the
/// [common vault errors](crate::commands#common-vault-errors).
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

/// Analyzes the document at `path` and returns a draft entry suggestion.
///
/// Requires the unlocked vault and a granted path: the file the user dropped
/// on a window. The size and type are checked from the file's metadata before
/// it is read, so an oversized drop is never loaded into memory. Nothing is
/// stored.
///
/// # Errors
///
/// Returns `path_not_granted` for a path the user never handed over;
/// `file_unreadable` when the file cannot be read; `file_too_large` (with the
/// cap as `max_mb`), `file_empty`, `file_type_unsupported` and
/// `name_required` when the document is refused; `not_found` when the entity
/// does not exist; and the
/// [common vault errors](crate::commands#common-vault-errors).
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
        let metadata = std::fs::metadata(&path).map_err(|err| {
            CommandError::desktop(
                DesktopError::FileUnreadable,
                format!("could not read dropped file: {err}"),
            )
        })?;
        let mime_type = oikonomia_core::documents::resolve_mime("", &filename);
        oikonomia_core::documents::validate_document_file(&filename, &mime_type, metadata.len())?;

        let data = std::fs::read(&path).map_err(|err| {
            CommandError::desktop(
                DesktopError::FileUnreadable,
                format!("could not read dropped file: {err}"),
            )
        })?;
        analyze_readonly(
            &vault, &model_dir, entity_id, &filename, &mime_type, &data, locale,
        )
    })
    .await
}

/// Analyzes a document held in memory and suggests a draft entry.
///
/// The vault is held only to read the entity and its accounts, and released
/// before the OCR pass, which can take seconds. Stores nothing.
///
/// # Errors
///
/// Returns the validation errors of a refused document, `not_found` when the
/// entity does not exist, and `vault_locked`, `io` or `vault_corrupt` from
/// the vault.
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
    let mime_type = oikonomia_core::documents::resolve_mime(mime_type, filename);
    oikonomia_core::documents::validate_document_file(filename, &mime_type, data.len() as u64)?;

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
        &mime_type,
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

/// A stored document's metadata and bytes, for the in-app viewer.
#[derive(Debug, Serialize)]
pub(crate) struct DocumentContent {
    /// The document's metadata.
    pub meta: DocumentMeta,
    /// The document's bytes as base64, the form IPC carries. Bounded by core's
    /// size cap on a stored document.
    pub data_base64: String,
}

/// Lists the documents stored for an entity, metadata only.
///
/// Requires the unlocked vault. An unknown entity has no documents, so it
/// yields an empty list, not an error.
///
/// # Errors
///
/// Returns `validation_internal` when a stored identifier cannot be parsed,
/// and the [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn document_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<DocumentMeta>> {
    with_connection(&state, move |conn| list_documents(conn, entity_id)).await
}

/// Returns one stored document, decrypted, for the in-app viewer.
///
/// Requires the unlocked vault. The bytes cross IPC only; nothing is written
/// to disk.
///
/// # Errors
///
/// Returns `not_found` when the document does not exist,
/// `validation_internal` when a stored identifier cannot be parsed, and the
/// [common vault errors](crate::commands#common-vault-errors).
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

/// Deletes a stored document, permanently. The entry it was attached to
/// stays.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the document does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn document_delete(
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<()> {
    with_connection(&state, move |conn| delete_document(conn, document_id)).await
}

/// Stores a document the webview picked and attaches it to a posted entry.
///
/// Requires the unlocked vault. The document is not analyzed: analysis
/// belongs to the flow that drafts a new entry, so the stored analysis stays
/// empty here.
///
/// # Errors
///
/// Returns `file_data_invalid` when `data_base64` is not base64; `not_found`
/// when the entry does not exist; `wrong_book` when it belongs to another
/// entity; `file_too_large` (with the cap as `max_mb`), `file_empty`,
/// `file_type_unsupported` and `name_required` when the document is refused;
/// `name_taken` when the entity already stores a document under the file
/// name; and the [common vault errors](crate::commands#common-vault-errors).
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

/// Exports a stored document, decrypted, to a path chosen in a native save
/// dialog. Returns the path written, or `None` if the user cancelled.
///
/// Requires the unlocked vault. Like the journal CSV export
/// ([`csv_export_journal`](crate::commands::csv_export_journal)) and the
/// report PDF ([`report_export_pdf`](crate::commands::report_export_pdf)),
/// this writes plaintext to disk, and like them only to a path the user
/// picked in the dialog ([`save_with_dialog`]).
///
/// # Errors
///
/// Returns `not_found` when the document does not exist,
/// `validation_internal` when a stored identifier cannot be parsed,
/// `save_location_invalid` when the dialog's answer is not a path,
/// `save_failed` when the file cannot be written, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn document_export(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<Option<String>> {
    let (document, data) =
        with_connection(&state, move |conn| get_document(conn, document_id)).await?;

    let target = SaveTarget {
        filter: None,
        file_name: document.filename,
        complete_path: std::convert::identity,
    };
    save_with_dialog(&app, target, data).await
}
