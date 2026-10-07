//! Journal entry commands: list, post, correct, hide and void.
//!
//! Every command here requires the unlocked vault. The entry forms post
//! through the simple-entry commands, which send a kind and the accounts for
//! its roles; core turns that into balanced lines. [`entry_post`] takes the
//! lines themselves.
//!
//! # Simple-entry errors
//!
//! Every command that posts a simple entry can return these, from core's
//! validation of the entry:
//!
//! - `amount_not_positive` for an amount of zero or less;
//! - `account_required` (with the role as `role`) when a role the kind needs
//!   has no account;
//! - `account_wrong_type` (with `role` and the account's `code`) when a role's account has
//!   a type the role does not accept;
//! - `bill_status_required` when a bill does not say whether it is paid;
//! - `same_account` when both sides name one account;
//! - `account_wrong_entity` when an account belongs to another entity;
//! - `account_inactive` when an account is inactive;
//! - `invalid_date` when the entry date is not a date;
//! - `not_found` when an account does not exist.

use crate::commands::support::{
    decode_document_base64, dropped_file_name, require_granted_path, run_blocking, with_connection,
    with_localized_connection,
};
use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::AppState;
use oikonomia_core::documents::post_simple_entry_with_document;
use oikonomia_core::domain::{AccountId, EntityId, JournalEntryId};
use oikonomia_core::ledger::{
    EntryFilter, PostJournal, PostJournalRequest, PostSimpleEntry, PostSimpleEntryRequest,
    PostedEntryView, VoidResult, get_entry, list_entries, post_entry, post_simple_entry,
    replace_simple_entry, set_entry_hidden, void_entry,
};
use oikonomia_core::util::DateText;
use tauri::State;

/// Lists an entity's posted entries, voided ones included, optionally
/// narrowed by text, by a date range and by account.
///
/// Requires the unlocked vault. An unknown entity has no entries, so it
/// yields an empty list, not an error.
///
/// # Errors
///
/// Returns `invalid_date` when `from` or `to` is not a date, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is one field of the IPC payload; tracked for the API pass"
)]
pub(crate) async fn entry_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: Option<DateText>,
    to: Option<DateText>,
    search: Option<String>,
    account_id: Option<AccountId>,
) -> CommandResult<Vec<PostedEntryView>> {
    with_connection(&state, move |conn| {
        let filter = EntryFilter {
            text: search,
            date_from: DateText::parse_optional(from.as_ref())?,
            date_to: DateText::parse_optional(to.as_ref())?,
            account_id,
        };
        list_entries(conn, entity_id, &filter)
    })
    .await
}

/// Returns one posted entry with its lines.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `not_found` when the entry does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_get(
    state: State<'_, AppState>,
    id: JournalEntryId,
) -> CommandResult<PostedEntryView> {
    with_connection(&state, move |conn| get_entry(conn, id)).await
}

/// Posts a journal entry given as explicit debit and credit lines.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `too_few_lines` for fewer than two lines, `invalid_line_amounts`
/// for a line that is not a debit or a credit but both or neither,
/// `negative_money` for a negative amount, `unbalanced_entry` when debits
/// and credits differ, `money_overflow` when a total does not fit the money
/// type, `invalid_date` when the entry date is not a date, `not_found` when
/// a line's account does not exist, `account_wrong_entity` when it belongs to
/// another entity, `account_inactive` when it is inactive, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_post(
    state: State<'_, AppState>,
    input: PostJournalRequest,
) -> CommandResult<PostedEntryView> {
    with_connection(&state, move |conn| {
        let input = PostJournal::try_from(input)?;
        post_entry(conn, &input)
    })
    .await
}

/// Posts an entry from the simple form: a kind, an amount and the accounts
/// for the kind's roles. Core builds the lines.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns the [simple-entry errors](self#simple-entry-errors) and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_post_simple(
    state: State<'_, AppState>,
    input: PostSimpleEntryRequest,
) -> CommandResult<PostedEntryView> {
    with_connection(&state, move |conn| {
        let input = PostSimpleEntry::try_from(input)?;
        post_simple_entry(conn, &input)
    })
    .await
}

/// Posts a simple entry and stores the document it was drafted from, in one
/// transaction.
///
/// Requires the unlocked vault. `data_base64` is the document the webview
/// picked; if either the entry or the document is refused, neither is
/// written.
///
/// # Errors
///
/// Returns `file_data_invalid` when `data_base64` is not base64;
/// `file_too_large` (with the cap as `max_mb`), `file_empty`,
/// `file_type_unsupported` and `name_required` when the document is refused;
/// `name_taken` when the entity already stores a document under the file
/// name; the [simple-entry errors](self#simple-entry-errors); and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is one field of the IPC payload; tracked for the API pass"
)]
pub(crate) async fn entry_post_simple_with_document(
    state: State<'_, AppState>,
    input: PostSimpleEntryRequest,
    filename: String,
    mime_type: String,
    data_base64: String,
    analysis_json: Option<String>,
) -> CommandResult<PostedEntryView> {
    let data = decode_document_base64(&data_base64)?;

    with_connection(&state, move |conn| {
        let input = PostSimpleEntry::try_from(input)?;
        let (view, _document) = post_simple_entry_with_document(
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

/// Posts a simple entry and stores the document at `path`, in one
/// transaction.
///
/// Requires the unlocked vault and a granted path: the file the user dropped
/// on a window. The file is read and validated again at post time, so one
/// that moved since the drop yields an error and nothing is written. The
/// document is stored under the name it was dropped as.
///
/// # Errors
///
/// Returns `path_not_granted` for a path the user never handed over;
/// `file_unreadable` when the file cannot be read; `file_too_large` (with the
/// cap as `max_mb`), `file_empty`, `file_type_unsupported` and
/// `name_required` when the document is refused; `name_taken` when the entity
/// already stores a document under the file name; the
/// [simple-entry errors](self#simple-entry-errors); and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_post_simple_with_document_path(
    state: State<'_, AppState>,
    input: PostSimpleEntryRequest,
    path: String,
    analysis_json: Option<String>,
) -> CommandResult<PostedEntryView> {
    let filename = dropped_file_name(&path);
    let grants = state.path_grants();
    let path = run_blocking(move || require_granted_path(&grants, &path)).await?;

    let vault = state.vault();
    state.touch();

    run_blocking(move || {
        // Reject oversized/unsupported files from metadata alone before reading.
        let metadata = std::fs::metadata(&path).map_err(|err| {
            CommandError::desktop(
                DesktopError::FileUnreadable,
                format!("could not read the dropped file: {err}"),
            )
        })?;
        let mime_type = oikonomia_core::documents::resolve_mime("", &filename);
        oikonomia_core::documents::validate_document_file(&filename, &mime_type, metadata.len())?;

        let data = std::fs::read(&path).map_err(|err| {
            CommandError::desktop(
                DesktopError::FileUnreadable,
                format!("could not read the dropped file: {err}"),
            )
        })?;

        let guard = vault.acquire();
        let conn = guard.connection()?;
        let input = PostSimpleEntry::try_from(input)?;
        let (view, _document) = post_simple_entry_with_document(
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

/// Corrects a posted entry: voids the original and posts the replacement in
/// one transaction.
///
/// Requires the unlocked vault. The replacement keeps the original's hidden
/// flag and takes over its documents. The reversal's description is written
/// in the app's stored language.
///
/// # Errors
///
/// Returns `not_found` when the original does not exist, `wrong_book` when
/// the replacement names another entity, `entry_already_voided` and
/// `entry_not_posted` when the original cannot be voided, the
/// [simple-entry errors](self#simple-entry-errors), and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_replace_simple(
    state: State<'_, AppState>,
    original_id: JournalEntryId,
    input: PostSimpleEntryRequest,
) -> CommandResult<PostedEntryView> {
    with_localized_connection(&state, move |conn, locale| {
        let input = PostSimpleEntry::try_from(input)?;
        replace_simple_entry(conn, original_id, &input, locale)
    })
    .await
}

/// Sets or clears the hidden flag on a posted entry and returns the entry.
///
/// Requires the unlocked vault. A hidden entry stays in the books and is left
/// out of the exports made for other people.
///
/// # Errors
///
/// Returns `not_found` when the entry does not exist, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_set_hidden(
    state: State<'_, AppState>,
    id: JournalEntryId,
    hidden: bool,
) -> CommandResult<PostedEntryView> {
    with_connection(&state, move |conn| set_entry_hidden(conn, id, hidden)).await
}

/// Voids a posted entry by posting its reversal on the same date.
///
/// Requires the unlocked vault. The reversal's description is written in the
/// app's stored language.
///
/// # Errors
///
/// Returns `not_found` when the entry does not exist, `entry_already_voided`
/// when it has been voided or is itself a reversal, `entry_not_posted` when
/// it is not a posted entry, and the
/// [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn entry_void(
    state: State<'_, AppState>,
    id: JournalEntryId,
) -> CommandResult<VoidResult> {
    with_localized_connection(&state, move |conn, locale| void_entry(conn, id, locale)).await
}
