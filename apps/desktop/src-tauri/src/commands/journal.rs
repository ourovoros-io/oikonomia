//! Journal entry commands.

use crate::commands::support::{
    await_blocking, decode_document_base64, dropped_file_name, require_granted_path,
    stored_text_locale, with_vault_blocking,
};
use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::AppState;
use oikonomia_core::documents::post_simple_entry_with_document;
use oikonomia_core::domain::{AccountId, EntityId, JournalEntryId};
use oikonomia_core::ledger::{
    EntryFilter, PostJournal, PostSimpleEntry, PostedEntryView, VoidResult, get_entry,
    list_entries, post_entry, post_simple_entry, replace_simple_entry, set_entry_hidden,
    void_entry,
};
use tauri::State;

/// List journal entries matching optional search/date/account filters.
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is one field of the IPC payload; tracked for the API pass"
)]
pub(crate) async fn entry_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: Option<String>,
    to: Option<String>,
    search: Option<String>,
    account_id: Option<AccountId>,
) -> CommandResult<Vec<PostedEntryView>> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        let filter = EntryFilter {
            text: search,
            date_from: from,
            date_to: to,
            account_id,
        };
        list_entries(conn, entity_id, &filter)
    })
    .await
}

/// Get one entry.
#[tauri::command]
pub(crate) async fn entry_get(
    state: State<'_, AppState>,
    id: JournalEntryId,
) -> CommandResult<PostedEntryView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        get_entry(conn, id)
    })
    .await
}

/// Post a balanced journal entry.
#[tauri::command]
pub(crate) async fn entry_post(
    state: State<'_, AppState>,
    input: PostJournal,
) -> CommandResult<PostedEntryView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        post_entry(conn, &input)
    })
    .await
}

/// Post a simple-form entry (kind + role accounts); line construction is in core.
#[tauri::command]
pub(crate) async fn entry_post_simple(
    state: State<'_, AppState>,
    input: PostSimpleEntry,
) -> CommandResult<PostedEntryView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        post_simple_entry(conn, &input)
    })
    .await
}

/// Post a simple entry together with its analyzed document (one transaction).
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is one field of the IPC payload; tracked for the API pass"
)]
pub(crate) async fn entry_post_simple_with_document(
    state: State<'_, AppState>,
    input: PostSimpleEntry,
    filename: String,
    mime_type: String,
    data_base64: String,
    analysis_json: Option<String>,
) -> CommandResult<PostedEntryView> {
    let data = decode_document_base64(&data_base64)?;

    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        let (view, _meta) = post_simple_entry_with_document(
            conn,
            &input,
            &filename,
            &mime_type,
            &data,
            analysis_json.as_deref(),
        )?;
        Ok(view)
    })
    .await
}

/// Post a simple entry with a document from a filesystem path (native drop).
/// The file is re-read and re-validated at post time; if it moved since the
/// drop, a clean error surfaces and nothing is written.
#[tauri::command]
pub(crate) async fn entry_post_simple_with_document_path(
    state: State<'_, AppState>,
    input: PostSimpleEntry,
    path: String,
    analysis_json: Option<String>,
) -> CommandResult<PostedEntryView> {
    let filename = dropped_file_name(&path);
    let path = require_granted_path(&state, &path)?;

    let vault = state.vault();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        // Reject oversized/unsupported files from metadata alone before reading.
        let meta = std::fs::metadata(&path).map_err(|e| {
            CommandError::desktop(
                DesktopError::FileUnreadable,
                format!("could not read the dropped file: {e}"),
            )
        })?;
        let mime = oikonomia_core::documents::resolve_mime("", &filename);
        oikonomia_core::documents::validate_document_file(&filename, &mime, meta.len())?;

        let data = std::fs::read(&path).map_err(|e| {
            CommandError::desktop(
                DesktopError::FileUnreadable,
                format!("could not read the dropped file: {e}"),
            )
        })?;

        let guard = vault.acquire();
        let conn = guard.connection()?;
        let (view, _meta) = post_simple_entry_with_document(
            conn,
            &input,
            &filename,
            &mime,
            &data,
            analysis_json.as_deref(),
        )?;
        Ok(view)
    }))
    .await
}

/// Correct a posted entry: void the original and post the replacement in one
/// transaction; attached documents follow the replacement.
#[tauri::command]
pub(crate) async fn entry_replace_simple(
    state: State<'_, AppState>,
    original_id: JournalEntryId,
    input: PostSimpleEntry,
) -> CommandResult<PostedEntryView> {
    let locale = stored_text_locale(&state);

    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        replace_simple_entry(conn, original_id, &input, locale)
    })
    .await
}

/// Set the owner-only hidden flag on an existing journal entry.
#[tauri::command]
pub(crate) async fn entry_set_hidden(
    state: State<'_, AppState>,
    id: JournalEntryId,
    hidden: bool,
) -> CommandResult<PostedEntryView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        set_entry_hidden(conn, id, hidden)
    })
    .await
}

/// Void an entry (posts reverse).
#[tauri::command]
pub(crate) async fn entry_void(
    state: State<'_, AppState>,
    id: JournalEntryId,
) -> CommandResult<VoidResult> {
    let locale = stored_text_locale(&state);

    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        void_entry(conn, id, locale)
    })
    .await
}
