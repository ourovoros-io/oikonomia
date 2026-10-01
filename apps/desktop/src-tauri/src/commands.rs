//! Tauri command handlers (thin wrappers over core + state).

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;
use base64::Engine;
use oikonomia_core::csv::{
    CsvImportPostInput, CsvImportPostResult, CsvImportPreview, CsvImportPreviewInput,
    default_journal_export_file_name, ensure_csv_path, export_journal_csv, post_import_rows,
    preview_bank_csv_file,
};
use oikonomia_core::documents::{
    AnalyzerStatus, DocumentId, DocumentMeta, DocumentSuggestion, analyze_document_bytes,
    analyzer_status, attach_document, delete_document, get_document, list_documents,
    post_simple_entry_with_document, suggest_accounts_for_entity,
};
use oikonomia_core::domain::{
    Account, AccountId, Entity, EntityId, JournalEntryId, RecurringTemplateId,
};
use oikonomia_core::error::Error as CoreError;
use oikonomia_core::ledger::{
    BalanceSheet, CashFlowSeries, CreateAccount, CreateEntity, CreateRecurringTemplate,
    DEFAULT_LOCK_TIMEOUT_SECS, DashboardSummary, EntryFilter, PnL, PostJournal, PostSimpleEntry,
    PostedEntryView, RecurringPostResult, RecurringTemplateView, RegisterLine, TrialBalance,
    UpdateAccount, UpdateRecurringTemplate, VoidResult, account_balance, account_register,
    activity_window, archive_account, archive_entity, balance_sheet, cash_flow_series,
    create_account, create_entity, create_recurring_template, dashboard_summary, delete_entity,
    delete_recurring_template, get_entity, get_entry, get_lock_timeout_secs,
    get_recurring_template, list_accounts, list_entities, list_entries, list_recurring_templates,
    post_entry, post_recurring_template, post_simple_entry, profit_and_loss,
    profit_and_loss_export, replace_simple_entry, set_account_opening_balance, set_entry_hidden,
    set_lock_timeout_secs, trial_balance, update_account, update_entity, update_recurring_template,
    void_entry,
};
use oikonomia_core::prefs::{
    LastRoleAccounts, Locale, UiPrefs, last_accounts_key, load_ui_prefs, save_ui_prefs,
};
use oikonomia_core::util::{format_date, utc_today};
use oikonomia_core::vault::{BACKUP_EXTENSION, Vault, VaultStatus, default_backup_file_name};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, State};
use zeroize::Zeroizing;

/// Where users send questions and bug reports. Every support pointer the app
/// shows derives from this one address.
pub(crate) const SUPPORT_EMAIL: &str = "info@ourovoros.io";

/// Static app metadata for the about screen / diagnostics.
#[derive(Debug, Serialize)]
pub struct AppInfo {
    /// Crate version.
    pub version: &'static str,
    /// Product name.
    pub name: &'static str,
    /// Support mailbox, shown verbatim so a user can copy it. Opening it is
    /// [`open_support_email`]'s job; the webview never builds the URL.
    pub support_email: &'static str,
}

// --- Vault -----------------------------------------------------------------

/// Return vault lock lifecycle status.
///
/// Async so the frontend's activity heartbeat never blocks the main thread,
/// even while a long operation (rekey, analysis save) holds the vault mutex.
#[tauri::command]
pub async fn vault_status(state: State<'_, AppState>) -> CommandResult<VaultStatus> {
    let vault = state.vault();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let guard = crate::state::lock_vault(&vault);
        Ok(guard.status())
    }))
    .await
}

/// Heartbeat for the idle watchdog. Separate from [`vault_status`] so lock
/// probes (quick-add focus) do not extend the idle window.
#[tauri::command]
pub fn vault_touch(state: State<'_, AppState>) {
    state.touch();
}

/// Create a new encrypted vault with the master password.
///
/// The password is wiped from memory when the command returns; only the
/// derived key lives on, inside `SQLCipher`.
#[tauri::command]
pub async fn vault_init(
    state: State<'_, AppState>,
    password: Zeroizing<String>,
) -> CommandResult<VaultStatus> {
    let status = with_vault_blocking(&state, move |vault| {
        vault.init(&password)?;
        Ok(vault.status())
    })
    .await?;
    state.sync_watchdog_gate(status);
    Ok(status)
}

/// Unlock an existing vault. The password is wiped when the command returns.
#[tauri::command]
pub async fn vault_unlock(
    state: State<'_, AppState>,
    password: Zeroizing<String>,
) -> CommandResult<VaultStatus> {
    let (status, secs) = with_vault_blocking(&state, move |vault| {
        vault.unlock(&password)?;

        // Refresh the watchdog's timeout cache from the now-readable settings.
        // A failed read falls back to the default, but never silently.
        let secs = match vault.connection().and_then(get_lock_timeout_secs) {
            Ok(secs) => secs,
            Err(err) => {
                log::warn!("could not read lock timeout after unlock, using default: {err}");
                DEFAULT_LOCK_TIMEOUT_SECS
            }
        };

        Ok((vault.status(), secs))
    })
    .await?;

    state.set_lock_timeout_cache(secs);
    state.sync_watchdog_gate(status);
    Ok(status)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::require_granted_path;
    use crate::state::AppState;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oiko-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        dir
    }

    #[test]
    fn webview_supplied_paths_need_a_native_grant() {
        let dir = temp_dir("path-grant");
        let state = AppState::open_path(dir.clone(), dir.clone()).expect("state");
        let archive = dir.join("books.oikonomia-backup");
        std::fs::write(&archive, b"OIKOBACK").expect("write");
        let text = archive.to_str().expect("utf-8 path");

        let refused = require_granted_path(&state, text).expect_err("ungranted path");
        assert_eq!(refused.code, "validation");

        state.grant_paths([archive.clone()]);
        assert_eq!(
            require_granted_path(&state, text).expect("granted path"),
            archive
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Change the master password (requires the current password). Both
/// passwords are wiped when the command returns.
#[tauri::command]
pub async fn vault_change_password(
    state: State<'_, AppState>,
    old: Zeroizing<String>,
    new: Zeroizing<String>,
) -> CommandResult<VaultStatus> {
    let vault = state.vault();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let mut guard = crate::state::lock_vault(&vault);
        guard.change_password(&old, &new)?;
        Ok(guard.status())
    }))
    .await
}

/// Lock the vault for this session.
#[tauri::command]
pub async fn vault_lock(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<VaultStatus> {
    let status = with_vault_blocking(&state, move |vault| {
        vault.lock();
        Ok(vault.status())
    })
    .await?;
    state.sync_watchdog_gate(status);
    let _ = app.emit("vault-locked", ());
    Ok(status)
}

/// Write a portable ciphertext archive. Does not lock; an unlocked session
/// stays unlocked.
///
/// Always presents a native save dialog (same pattern as [`document_export`]).
/// The suggested filename is `oikonomia-backup-YYYY-MM-DD.oikonomia-backup`
/// using the local calendar date. An unlocked vault is snapshotted with
/// `VACUUM INTO` so the copy is consistent without closing `SQLCipher`.
/// The archive is `vault.db` plus `vault.header.json` only: it is not
/// re-encrypted and never stores the master password.
///
/// Returns the destination path, or `None` if the user cancelled.
#[tauri::command]
pub async fn vault_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Option<String>> {
    let file_name = default_backup_file_name();
    let filter_label = crate::tray::backup_filter_label(load_ui_prefs(state.data_dir()).locale);
    let picked = await_blocking(tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            use tauri_plugin_dialog::DialogExt;
            Ok(app
                .dialog()
                .file()
                .add_filter(filter_label, &[BACKUP_EXTENSION])
                .set_file_name(&file_name)
                .blocking_save_file())
        }
    }))
    .await?;

    let Some(file_path) = picked else {
        return Ok(None);
    };
    let dest = with_backup_extension(file_path.into_path().map_err(|e| CommandError {
        code: "io".into(),
        message: format!("invalid save location: {e}"),
    })?);

    with_vault_blocking(&state, move |vault| {
        vault.backup_to(&dest)?;
        Ok(dest.display().to_string())
    })
    .await
    .map(Some)
}

/// Restore a portable vault archive and leave the vault locked.
///
/// `path` is the archive to unpack. When `path` is `None`, a native open
/// dialog chooses the file (the in-app path). A concrete path is accepted only
/// if [`vault_pick_backup`] returned it, so the webview cannot name arbitrary
/// files. Decrypt is not performed; the owner unlocks afterwards with the
/// existing master password.
///
/// Existing vault files are not overwritten unless `replace` is `true`.
/// An uninitialized data directory accepts `replace: false`.
///
/// Returns the archive path that was restored, or `None` if the user cancelled
/// the open dialog.
#[tauri::command]
pub async fn vault_restore(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
    replace: bool,
) -> CommandResult<Option<String>> {
    // Refuse an ungranted path before touching the session.
    let granted = path
        .map(|chosen| require_granted_path(&state, &chosen))
        .transpose()?;
    lock_vault_session(&app, &state).await?;

    let archive = if let Some(chosen) = granted {
        chosen
    } else {
        let Some(picked) = pick_backup_path(&app, &state).await? else {
            return Ok(None);
        };
        picked
    };

    with_vault_blocking(&state, move |vault| {
        vault.restore_from(&archive, replace)?;
        Ok(archive.display().to_string())
    })
    .await
    .map(Some)
}

/// Choose a backup file via a native open dialog.
///
/// Read-only: does not restore, lock, or write vault files. Returns the chosen
/// path, or `None` if the user cancelled. The frontend confirms, then calls
/// [`vault_restore`] with that path and `replace`.
#[tauri::command]
pub async fn vault_pick_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Option<String>> {
    let Some(path) = pick_backup_path(&app, &state).await? else {
        return Ok(None);
    };
    Ok(Some(path.display().to_string()))
}

/// Native Open dialog for a `.oikonomia-backup` file. `None` if cancelled.
/// The chosen path is granted so [`vault_restore`] may receive it back.
async fn pick_backup_path(
    app: &tauri::AppHandle,
    state: &AppState,
) -> CommandResult<Option<PathBuf>> {
    let filter_label = crate::tray::backup_filter_label(load_ui_prefs(state.data_dir()).locale);
    let picked = await_blocking(tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            use tauri_plugin_dialog::DialogExt;
            Ok(app
                .dialog()
                .file()
                .add_filter(filter_label, &[BACKUP_EXTENSION])
                .blocking_pick_file())
        }
    }))
    .await?;
    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path.into_path().map_err(|e| CommandError {
        code: "io".into(),
        message: format!("invalid backup location: {e}"),
    })?;
    state.grant_paths([path.clone()]);
    Ok(Some(path))
}

/// Close any open `SQLCipher` connection and notify the UI when the session
/// actually transitioned from unlocked to locked.
async fn lock_vault_session(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
) -> CommandResult<()> {
    let was_unlocked = with_vault_blocking(state, |vault| {
        let was_unlocked = vault.status() == VaultStatus::Unlocked;
        vault.lock();
        Ok(was_unlocked)
    })
    .await?;
    // Always park: restore (and any other session lock) leaves the vault
    // closed even when it was already locked.
    state.sync_watchdog_gate(VaultStatus::Locked);
    if was_unlocked {
        let _ = app.emit("vault-locked", ());
    }
    Ok(())
}

fn with_backup_extension(path: std::path::PathBuf) -> std::path::PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext == BACKUP_EXTENSION => path,
        _ => {
            let mut name = path.file_name().map_or_else(
                || std::ffi::OsString::from("oikonomia"),
                std::ffi::OsString::from,
            );
            name.push(".");
            name.push(BACKUP_EXTENSION);
            match path.parent().filter(|p| !p.as_os_str().is_empty()) {
                Some(parent) => parent.join(name),
                None => std::path::PathBuf::from(name),
            }
        }
    }
}

/// Return build identity and the support address (no secrets).
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        name: "Oikonomia",
        support_email: SUPPORT_EMAIL,
    }
}

/// Open the default mail client on the support mailbox.
///
/// The URL is built here from [`SUPPORT_EMAIL`] and handed to the opener's
/// Rust API, which applies no capability scope. That is deliberate: the
/// webview never supplies a URL, so `capabilities/default.json` needs no
/// `mailto:` glob, and a glob that could admit extra recipients never exists.
#[tauri::command]
pub fn open_support_email(app: tauri::AppHandle) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;

    app.opener()
        .open_url(support_mailto(env!("CARGO_PKG_VERSION")), None::<&str>)
        .map_err(|e| CommandError {
            code: "io".into(),
            message: format!("could not open the mail client: {e}"),
        })
}

/// `mailto:` link to [`SUPPORT_EMAIL`] whose subject names the app version,
/// so every support thread opens with the one fact each report needs.
fn support_mailto(version: &str) -> String {
    let subject = percent_encode(&format!("Oikonomia v{version} support"));
    format!("mailto:{SUPPORT_EMAIL}?subject={subject}")
}

/// RFC 3986 percent-encoding for a `mailto:` query value: unreserved bytes
/// pass through, everything else becomes `%XX`.
fn percent_encode(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }
    out
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod app_info_tests {
    use super::{SUPPORT_EMAIL, app_info, percent_encode, support_mailto};

    #[test]
    fn app_info_carries_the_support_address_but_never_a_url() {
        let json = serde_json::to_value(app_info()).expect("serialize");
        assert_eq!(json["support_email"], "info@ourovoros.io");
        // The webview displays the address; only Rust turns it into a URL.
        assert!(json.get("support_mailto").is_none());
    }

    #[test]
    fn support_mailto_targets_the_support_mailbox_with_the_version() {
        let mailto = support_mailto("1.2.3");
        assert!(mailto.starts_with(&format!("mailto:{SUPPORT_EMAIL}?")));
        assert_eq!(
            mailto,
            "mailto:info@ourovoros.io?subject=Oikonomia%20v1.2.3%20support"
        );
    }

    #[test]
    fn percent_encode_keeps_unreserved_bytes_and_escapes_the_rest() {
        assert_eq!(percent_encode("a-b.c_d~1"), "a-b.c_d~1");
        assert_eq!(percent_encode("a b&c=d?é"), "a%20b%26c%3Dd%3F%C3%A9");
    }
}

// --- Entities --------------------------------------------------------------

/// List non-archived entities.
#[tauri::command]
pub async fn entity_list(state: State<'_, AppState>) -> CommandResult<Vec<Entity>> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        list_entities(conn)
    })
    .await
}

/// Create entity with chart template.
#[tauri::command]
pub async fn entity_create(
    state: State<'_, AppState>,
    input: CreateEntity,
) -> CommandResult<Entity> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        create_entity(conn, &input)
    })
    .await
}

/// Rename entity.
#[tauri::command]
pub async fn entity_update(
    state: State<'_, AppState>,
    id: EntityId,
    name: String,
) -> CommandResult<Entity> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        update_entity(conn, id, &name)
    })
    .await
}

/// Archive entity (soft-hide).
#[tauri::command]
pub async fn entity_archive(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        archive_entity(conn, id)
    })
    .await
}

/// Permanently delete an entity and all of its books data.
#[tauri::command]
pub async fn entity_delete(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        delete_entity(conn, id)
    })
    .await
}

// --- Accounts --------------------------------------------------------------

/// List accounts for an entity.
#[tauri::command]
pub async fn account_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<Account>> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        list_accounts(conn, entity_id)
    })
    .await
}

/// Create account.
#[tauri::command]
pub async fn account_create(
    state: State<'_, AppState>,
    input: CreateAccount,
) -> CommandResult<Account> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        create_account(conn, &input)
    })
    .await
}

/// Update account.
#[tauri::command]
pub async fn account_update(
    state: State<'_, AppState>,
    input: UpdateAccount,
) -> CommandResult<Account> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        update_account(conn, &input)
    })
    .await
}

/// Archive (deactivate) account.
#[tauri::command]
pub async fn account_archive(state: State<'_, AppState>, id: AccountId) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        archive_account(conn, id)
    })
    .await
}

/// Account register.
#[tauri::command]
pub async fn account_register_cmd(
    state: State<'_, AppState>,
    account_id: AccountId,
    from: Option<String>,
    to: Option<String>,
) -> CommandResult<Vec<RegisterLine>> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        account_register(conn, account_id, from.as_deref(), to.as_deref())
    })
    .await
}

/// Signed normal balance of one account as of a date.
#[tauri::command]
pub async fn account_balance_cmd(
    state: State<'_, AppState>,
    account_id: AccountId,
    as_of: String,
) -> CommandResult<i64> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        account_balance(conn, account_id, &as_of)
    })
    .await
}

/// Set an account's balance as of a date by posting the difference against
/// the book's Opening Balances equity account.
#[tauri::command]
pub async fn account_set_opening_balance(
    state: State<'_, AppState>,
    account_id: AccountId,
    target_minor: i64,
    as_of: String,
) -> CommandResult<PostedEntryView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        set_account_opening_balance(conn, account_id, target_minor, &as_of)
    })
    .await
}

// --- Journal ---------------------------------------------------------------

/// List journal entries matching optional search/date/account filters.
#[tauri::command]
pub async fn entry_list(
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
pub async fn entry_get(
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
pub async fn entry_post(
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
pub async fn entry_post_simple(
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
pub async fn entry_post_simple_with_document(
    state: State<'_, AppState>,
    input: PostSimpleEntry,
    filename: String,
    mime_type: String,
    data_base64: String,
    analysis_json: Option<String>,
) -> CommandResult<PostedEntryView> {
    // Base64 inflates by 4/3: reject oversized picks before decoding so a huge
    // file cannot balloon memory (same gate as document_analyze).
    let max_base64_len = oikonomia_core::documents::MAX_DOCUMENT_BYTES / 3 * 4 + 4;
    if data_base64.len() > max_base64_len {
        return Err(CommandError {
            code: "validation".into(),
            message: "file too large (max 8 MB)".into(),
        });
    }

    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64.trim())
        .map_err(|e| CommandError {
            code: "validation".into(),
            message: format!("invalid file data: {e}"),
        })?;

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
pub async fn entry_post_simple_with_document_path(
    state: State<'_, AppState>,
    input: PostSimpleEntry,
    path: String,
    analysis_json: Option<String>,
) -> CommandResult<PostedEntryView> {
    let path = require_granted_path(&state, &path)?;

    let vault = state.vault();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("document")
            .to_owned();

        // Reject oversized/unsupported files from metadata alone before reading.
        let meta = std::fs::metadata(&path).map_err(|e| CommandError {
            code: "io".into(),
            message: format!("could not read the dropped file: {e}"),
        })?;
        let mime = oikonomia_core::documents::resolve_mime("", &filename);
        oikonomia_core::documents::validate_document_file(&filename, &mime, meta.len())?;

        let data = std::fs::read(&path).map_err(|e| CommandError {
            code: "io".into(),
            message: format!("could not read the dropped file: {e}"),
        })?;

        let guard = crate::state::lock_vault(&vault);
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
pub async fn entry_replace_simple(
    state: State<'_, AppState>,
    original_id: JournalEntryId,
    input: PostSimpleEntry,
) -> CommandResult<PostedEntryView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        replace_simple_entry(conn, original_id, &input)
    })
    .await
}

/// Set the owner-only hidden flag on an existing journal entry.
#[tauri::command]
pub async fn entry_set_hidden(
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
pub async fn entry_void(
    state: State<'_, AppState>,
    id: JournalEntryId,
) -> CommandResult<VoidResult> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        void_entry(conn, id)
    })
    .await
}

// --- Recurring templates (local vault only; no auto-post) ------------------

/// List recurring templates for an entity (`due` when `next_date` ≤ UTC today).
#[tauri::command]
pub async fn recurring_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<RecurringTemplateView>> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        list_recurring_templates(conn, entity_id)
    })
    .await
}

/// Fetch one template.
#[tauri::command]
pub async fn recurring_get(
    state: State<'_, AppState>,
    id: RecurringTemplateId,
) -> CommandResult<RecurringTemplateView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        get_recurring_template(conn, id)
    })
    .await
}

/// Create a local recurring template.
#[tauri::command]
pub async fn recurring_create(
    state: State<'_, AppState>,
    input: CreateRecurringTemplate,
) -> CommandResult<RecurringTemplateView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        create_recurring_template(conn, &input)
    })
    .await
}

/// Replace mutable fields on a template.
#[tauri::command]
pub async fn recurring_update(
    state: State<'_, AppState>,
    input: UpdateRecurringTemplate,
) -> CommandResult<RecurringTemplateView> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        update_recurring_template(conn, &input)
    })
    .await
}

/// Delete a template. Posted journal entries are left intact.
#[tauri::command]
pub async fn recurring_delete(
    state: State<'_, AppState>,
    id: RecurringTemplateId,
) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        delete_recurring_template(conn, id)
    })
    .await
}

/// Post one journal entry from a template, then advance `next_date`.
///
/// Defaults: `entry_date` = template `next_date`, `amount_minor` = template
/// amount. Overrides apply only to this post (confirm-sheet adjust-before-save);
/// cadence still steps from the stored `next_date`.
#[tauri::command]
pub async fn recurring_post(
    state: State<'_, AppState>,
    id: RecurringTemplateId,
    entry_date: Option<String>,
    amount_minor: Option<i64>,
) -> CommandResult<RecurringPostResult> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        post_recurring_template(conn, id, entry_date.as_deref(), amount_minor)
    })
    .await
}

// --- CSV import / export ---------------------------------------------------

/// Parse a bank CSV into suggested simple-entry rows. **Does not post.**
///
/// When `input.path` is omitted, a native Open dialog chooses the file.
/// `input.mapping` overrides header auto-detect when set.
/// Returns `None` if the user cancelled the dialog.
#[tauri::command]
pub async fn csv_import_preview(
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
pub async fn csv_import_post(
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
pub async fn csv_export_journal(
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

    let picked = await_blocking(tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            use tauri_plugin_dialog::DialogExt;
            Ok(app
                .dialog()
                .file()
                .add_filter("CSV", &["csv"])
                .set_file_name(&file_name)
                .blocking_save_file())
        }
    }))
    .await?;

    let Some(file_path) = picked else {
        return Ok(None);
    };
    let dest = ensure_csv_path(file_path.into_path().map_err(|e| CommandError {
        code: "io".into(),
        message: format!("invalid save location: {e}"),
    })?);

    std::fs::write(&dest, csv_text.as_bytes()).map_err(|e| CommandError {
        code: "io".into(),
        message: format!("could not save CSV: {e}"),
    })?;

    Ok(Some(dest.display().to_string()))
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
    let path = file_path.into_path().map_err(|e| CommandError {
        code: "io".into(),
        message: format!("invalid CSV location: {e}"),
    })?;
    state.grant_paths([path.clone()]);
    Ok(Some(path))
}

// --- Reports ---------------------------------------------------------------

/// Trial balance.
#[tauri::command]
pub async fn report_trial_balance(
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
pub async fn report_pnl(
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
pub async fn report_pnl_export(
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
pub async fn report_balance_sheet(
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

/// Client-built PDF bytes; native Save dialog writes them. No vault, no write gate.
#[tauri::command]
pub async fn report_export_pdf(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    bytes_base64: String,
    suggested_name: Option<String>,
) -> CommandResult<Option<String>> {
    let data = decode_pdf_export_bytes(&bytes_base64)?;
    state.touch();

    let file_name = pdf_export_file_name(suggested_name.as_deref());
    let picked = await_blocking(tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        Ok(app
            .dialog()
            .file()
            .add_filter("PDF", &["pdf"])
            .set_file_name(&file_name)
            .blocking_save_file())
    }))
    .await?;

    let Some(file_path) = picked else {
        return Ok(None);
    };
    let dest = ensure_pdf_path(file_path.into_path().map_err(|e| CommandError {
        code: "io".into(),
        message: format!("invalid save location: {e}"),
    })?);

    std::fs::write(&dest, &data).map_err(|e| CommandError {
        code: "io".into(),
        message: format!("could not save PDF: {e}"),
    })?;

    Ok(Some(dest.display().to_string()))
}

/// Decoded PDF cap for a monthly expense report (webview-generated).
const MAX_PDF_EXPORT_BYTES: usize = 32 * 1024 * 1024;

fn decode_pdf_export_bytes(bytes_base64: &str) -> CommandResult<Vec<u8>> {
    decode_capped_base64(bytes_base64, MAX_PDF_EXPORT_BYTES)
}

/// Reject on inflated base64 length before decode so a huge payload cannot balloon memory.
fn decode_capped_base64(bytes_base64: &str, max_decoded: usize) -> CommandResult<Vec<u8>> {
    let trimmed = bytes_base64.trim();
    let max_base64_len = max_decoded / 3 * 4 + 4;
    if trimmed.len() > max_base64_len {
        return Err(pdf_too_large_error(max_decoded));
    }

    let data = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|e| CommandError {
            code: "validation".into(),
            message: format!("invalid file data: {e}"),
        })?;

    if data.len() > max_decoded {
        return Err(pdf_too_large_error(max_decoded));
    }
    Ok(data)
}

fn pdf_too_large_error(max_decoded: usize) -> CommandError {
    CommandError {
        code: "validation".into(),
        message: format!("PDF too large (max {} MB)", max_decoded / (1024 * 1024)),
    }
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod pdf_export_tests {
    use super::{
        decode_capped_base64, decode_pdf_export_bytes, ensure_pdf_path, pdf_export_file_name,
    };
    use base64::Engine;

    #[test]
    fn decode_pdf_export_rejects_invalid_base64() {
        let err = decode_pdf_export_bytes("not-valid-base64!!!").expect_err("invalid");
        assert_eq!(err.code, "validation");
    }

    #[test]
    fn decode_pdf_export_rejects_oversized_before_decode() {
        // max_decoded=2 → max_base64 = 4; six chars must fail before decode.
        let err = decode_capped_base64("AAAAAA", 2).expect_err("cap");
        assert_eq!(err.code, "validation");
        assert!(err.message.contains("large"), "{}", err.message);
    }

    #[test]
    fn decode_pdf_export_rejects_oversized_after_decode() {
        // max_decoded=3 → max_base64 = 8; 4 decoded bytes encode to 8 chars.
        let encoded = base64::engine::general_purpose::STANDARD.encode([1_u8, 2, 3, 4]);
        assert_eq!(encoded.len(), 8);
        let err = decode_capped_base64(&encoded, 3).expect_err("after decode");
        assert_eq!(err.code, "validation");
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

/// Dashboard summary.
#[tauri::command]
pub async fn dashboard_summary_cmd(
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
pub async fn cash_flow_series_cmd(
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

// --- Settings --------------------------------------------------------------

/// Get auto-lock timeout seconds.
#[tauri::command]
pub async fn settings_get_lock_timeout(state: State<'_, AppState>) -> CommandResult<u64> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        get_lock_timeout_secs(conn)
    })
    .await
}

/// Set auto-lock timeout seconds.
#[tauri::command]
pub async fn settings_set_lock_timeout(state: State<'_, AppState>, secs: u64) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        set_lock_timeout_secs(conn, secs)
    })
    .await?;
    state.set_lock_timeout_cache(secs);
    Ok(())
}

/// Get the native UI locale. Plaintext preference: readable before unlock so
/// tray chrome and dialogs match the user's language before a password.
#[tauri::command]
pub fn settings_get_locale(state: State<'_, AppState>) -> Locale {
    load_ui_prefs(state.data_dir()).locale
}

/// Persist the native UI locale, then rebuild the tray menu and refresh the
/// quick-add window title when that window exists.
#[tauri::command]
pub fn settings_set_locale(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    locale: Locale,
) -> CommandResult<()> {
    let prefs_guard = state.lock_prefs();
    let mut prefs = load_ui_prefs(state.data_dir());
    prefs.locale = locale;
    save_ui_prefs(state.data_dir(), &prefs)?;
    drop(prefs_guard);
    crate::tray::apply_locale(&app, locale);
    Ok(())
}

/// Full plaintext UI prefs (locale, tray last-used). Safe before unlock.
#[tauri::command]
pub fn settings_get_ui_prefs(state: State<'_, AppState>) -> UiPrefs {
    load_ui_prefs(state.data_dir())
}

/// Remember last entity + role accounts after a successful tray post.
#[tauri::command]
pub fn settings_remember_quick_add(
    state: State<'_, AppState>,
    entity_id: String,
    kind: String,
    accounts: LastRoleAccounts,
) -> CommandResult<()> {
    let _guard = state.lock_prefs();
    let mut prefs = load_ui_prefs(state.data_dir());
    prefs.last_entity_id = Some(entity_id.clone());
    prefs
        .last_accounts_by_entity_kind
        .insert(last_accounts_key(&entity_id, &kind), accounts);
    save_ui_prefs(state.data_dir(), &prefs)?;
    Ok(())
}

#[tauri::command]
pub fn open_main_window(app: tauri::AppHandle) {
    crate::tray::show_main_window(&app);
}

#[tauri::command]
pub fn quick_add_hide(app: tauri::AppHandle) {
    crate::tray::hide_quick_add(&app);
}

// --- Documents / bill scan (bundled offline OCR) ---------------------------

/// Whether the shipped on-device OCR models are available.
#[tauri::command]
pub fn document_analyzer_status(state: State<'_, AppState>) -> AnalyzerStatus {
    analyzer_status(Some(state.ocr_model_dir().as_path()))
}

/// Store a dropped file in the encrypted vault and return a draft entry suggestion.
///
/// Analysis is fully offline (bundled OCR + heuristics). Nothing is sent to the network.
#[tauri::command]
pub async fn document_analyze(
    state: State<'_, AppState>,
    entity_id: EntityId,
    filename: String,
    mime_type: String,
    data_base64: String,
) -> CommandResult<DocumentSuggestion> {
    // Base64 inflates by 4/3: reject oversized picks before decoding so a huge
    // file cannot balloon memory (the drop path gates on fs metadata the same way).
    let max_base64_len = oikonomia_core::documents::MAX_DOCUMENT_BYTES / 3 * 4 + 4;
    if data_base64.len() > max_base64_len {
        return Err(CommandError {
            code: "validation".into(),
            message: "file too large (max 8 MB)".into(),
        });
    }

    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64.trim())
        .map_err(|e| CommandError {
            code: "validation".into(),
            message: format!("invalid file data: {e}"),
        })?;

    let vault = state.vault();
    let model_dir = state.ocr_model_dir().clone();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        analyze_readonly(&vault, &model_dir, entity_id, &filename, &mime_type, &data)
    }))
    .await
}

/// Analyze a file from a filesystem path (Tauri native drag-and-drop).
#[tauri::command]
pub async fn document_analyze_path(
    state: State<'_, AppState>,
    entity_id: EntityId,
    path: String,
) -> CommandResult<DocumentSuggestion> {
    let path = require_granted_path(&state, &path)?;

    let vault = state.vault();
    let model_dir = state.ocr_model_dir().clone();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("document")
            .to_owned();

        // Reject oversized/unsupported drops from metadata alone — a stray
        // 10 GB drop must not be read into memory before failing the size cap.
        let meta = std::fs::metadata(&path).map_err(|e| CommandError {
            code: "io".into(),
            message: format!("could not read dropped file: {e}"),
        })?;
        let mime = oikonomia_core::documents::resolve_mime("", &filename);
        oikonomia_core::documents::validate_document_file(&filename, &mime, meta.len())?;

        let data = std::fs::read(&path).map_err(|e| CommandError {
            code: "io".into(),
            message: format!("could not read dropped file: {e}"),
        })?;
        analyze_readonly(&vault, &model_dir, entity_id, &filename, &mime, &data)
    }))
    .await
}

/// Accept a webview-supplied path only if the user handed it to the app
/// through a native drop or dialog ([`AppState::grant_paths`]).
fn require_granted_path(state: &AppState, path: &str) -> CommandResult<PathBuf> {
    let path = PathBuf::from(path);
    if state.path_is_granted(&path) {
        Ok(path)
    } else {
        Err(CommandError {
            code: "validation".into(),
            message: "file path was not chosen through the app".into(),
        })
    }
}

/// Run vault work on the blocking pool: no command ever waits for the vault
/// mutex on the main thread or an async runtime worker (e.g. while a rekey
/// holds it for seconds).
async fn with_vault_blocking<T, F>(state: &State<'_, AppState>, f: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Vault) -> Result<T, CoreError> + Send + 'static,
{
    let vault = state.vault();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let mut guard = crate::state::lock_vault(&vault);
        f(&mut guard).map_err(CommandError::from)
    }))
    .await
}

/// Map a blocking-task join failure into a command error.
async fn await_blocking<T>(
    handle: tauri::async_runtime::JoinHandle<CommandResult<T>>,
) -> CommandResult<T> {
    match handle.await {
        Ok(result) => result,
        Err(err) => Err(CommandError {
            code: "io".into(),
            message: format!("background task failed: {err}"),
        }),
    }
}

/// Analyze a document in memory and suggest a draft entry. Persists
/// nothing: the file is stored only when the entry is posted
/// (`entry_post_simple_with_document`), keeping the no-orphan invariant.
fn analyze_readonly(
    vault: &Mutex<Vault>,
    model_dir: &Path,
    entity_id: EntityId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
) -> CommandResult<DocumentSuggestion> {
    let mime = oikonomia_core::documents::resolve_mime(mime_type, filename);
    oikonomia_core::documents::validate_document_file(filename, &mime, data.len() as u64)?;

    let (accounts, entity) = {
        let guard = crate::state::lock_vault(vault);
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
        &accounts,
        &entity.base_currency,
        Some(model_dir),
    )?;

    Ok(suggestion)
}

/// Metadata plus base64 payload for the in-app viewer.
#[derive(Debug, Serialize)]
pub struct DocumentContent {
    /// Metadata.
    pub meta: DocumentMeta,
    /// Raw bytes, base64-encoded for IPC (bounded by the 8 MiB cap).
    pub data_base64: String,
}

/// All stored documents for an entity (metadata only).
#[tauri::command]
pub async fn document_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<DocumentMeta>> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        list_documents(conn, entity_id)
    })
    .await
}

/// One document's bytes for the in-app viewer. Decrypted content crosses
/// IPC only; nothing is written to disk.
#[tauri::command]
pub async fn document_get(
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<DocumentContent> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
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
pub async fn document_delete(
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        delete_document(conn, document_id)
    })
    .await
}

/// Attach a file to an existing posted entry. No OCR pass — analysis only
/// runs on the drop-zone flow; `analysis_json` stays NULL here.
#[tauri::command]
pub async fn document_attach(
    state: State<'_, AppState>,
    entity_id: EntityId,
    entry_id: JournalEntryId,
    filename: String,
    mime_type: String,
    data_base64: String,
) -> CommandResult<DocumentMeta> {
    // Base64 inflates by 4/3: reject oversized picks before decoding so a huge
    // file cannot balloon memory (same gate as document_analyze).
    let max_base64_len = oikonomia_core::documents::MAX_DOCUMENT_BYTES / 3 * 4 + 4;
    if data_base64.len() > max_base64_len {
        return Err(CommandError {
            code: "validation".into(),
            message: "file too large (max 8 MB)".into(),
        });
    }

    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64.trim())
        .map_err(|e| CommandError {
            code: "validation".into(),
            message: format!("invalid file data: {e}"),
        })?;

    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        attach_document(conn, entity_id, entry_id, &filename, &mime_type, &data)
    })
    .await
}

/// Export a document to a user-chosen path. This is the only path by which
/// decrypted bytes reach disk, and it always goes through an explicit
/// native save dialog.
#[tauri::command]
pub async fn document_export(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    let (meta, data) = with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        get_document(conn, document_id)
    })
    .await?;

    // The blocking dialog must stay off the async runtime workers.
    let picked = await_blocking(tauri::async_runtime::spawn_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_file_name(meta.filename.as_str())
            .blocking_save_file())
    }))
    .await?;

    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path.into_path().map_err(|e| CommandError {
        code: "io".into(),
        message: format!("invalid save location: {e}"),
    })?;

    std::fs::write(&path, &data).map_err(|e| CommandError {
        code: "io".into(),
        message: format!("could not save file: {e}"),
    })?;

    Ok(Some(path.display().to_string()))
}
