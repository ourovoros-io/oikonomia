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
use oikonomia_core::domain::{Account, AccountId, Entity, EntityId, JournalEntryId};
use oikonomia_core::error::Error as CoreError;
use oikonomia_core::ledger::{
    BalanceSheet, CreateAccount, CreateEntity, DEFAULT_LOCK_TIMEOUT_SECS, DashboardSummary,
    EntryFilter, PnL, PostJournal, PostSimpleEntry, PostedEntryView, RegisterLine, TrialBalance,
    UpdateAccount, VoidResult, account_balance, account_register, archive_account, archive_entity,
    balance_sheet, create_account, create_entity_allowed, dashboard_summary, delete_entity,
    get_entity, get_entry, get_lock_timeout_secs, list_accounts, list_entities, list_entries,
    post_entry, post_simple_entry, profit_and_loss, replace_simple_entry,
    set_account_opening_balance, set_entry_hidden, set_lock_timeout_secs, trial_balance,
    update_account, update_entity, void_entry,
};
use oikonomia_core::license::{
    LicenseStatus, LicenseVerifier, install_license, record_trial_start, require_writes_allowed,
};
use oikonomia_core::prefs::{
    LastRoleAccounts, Locale, Theme, UiPrefs, last_accounts_key, load_ui_prefs, save_ui_prefs,
};
use oikonomia_core::vault::{BACKUP_EXTENSION, Vault, VaultStatus, default_backup_file_name};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use tauri::{Emitter, State};

/// Static app metadata for the about screen / diagnostics.
#[derive(Debug, Serialize)]
pub struct AppInfo {
    /// Crate version.
    pub version: &'static str,
    /// Product name.
    pub name: &'static str,
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
#[tauri::command]
pub async fn vault_init(
    state: State<'_, AppState>,
    password: String,
) -> CommandResult<VaultStatus> {
    let status = with_vault_blocking(&state, move |vault| {
        vault.init(&password)?;
        Ok(vault.status())
    })
    .await?;
    stamp_trial_start(&state)?;
    Ok(status)
}

/// Unlock an existing vault.
#[tauri::command]
pub async fn vault_unlock(
    state: State<'_, AppState>,
    password: String,
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

    stamp_trial_start(&state)?;
    state.set_lock_timeout_cache(secs);
    Ok(status)
}

/// Stamp `trial_started_at` once after a successful vault init or unlock.
fn stamp_trial_start(state: &AppState) -> CommandResult<()> {
    let _guard = state.lock_prefs();
    record_trial_start(state.data_dir())?;
    Ok(())
}

/// Change the master password (requires the current password).
#[tauri::command]
pub async fn vault_change_password(
    state: State<'_, AppState>,
    old: String,
    new: String,
) -> CommandResult<VaultStatus> {
    require_writes(&state)?;
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
/// dialog chooses the file (the in-app path). A concrete path is accepted so
/// a caller that already picked via [`vault_pick_backup`] can pass it through.
/// Decrypt is not performed; the owner unlocks afterwards with the existing
/// master password.
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
    lock_vault_session(&app, &state).await?;

    let archive = if let Some(chosen) = path {
        std::path::PathBuf::from(chosen)
    } else {
        let Some(picked) = pick_backup_path(&app, load_ui_prefs(state.data_dir()).locale).await?
        else {
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
    let Some(path) = pick_backup_path(&app, load_ui_prefs(state.data_dir()).locale).await? else {
        return Ok(None);
    };
    Ok(Some(path.display().to_string()))
}

/// Offline license / trial status. Never contacts the network.
#[tauri::command]
pub fn license_status(state: State<'_, AppState>) -> CommandResult<LicenseStatus> {
    let verifier = LicenseVerifier::production()?;
    Ok(oikonomia_core::license::license_status(
        state.data_dir(),
        &verifier,
    )?)
}

/// Native Open for a `.lic` file; verify, then atomically copy as `license.lic`.
///
/// Returns the new status, or `None` if the user cancelled. The stored file is
/// the signed original, not an unsigned cache.
#[tauri::command]
pub async fn license_install(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Option<LicenseStatus>> {
    let filter_label = crate::tray::license_filter_label(load_ui_prefs(state.data_dir()).locale);
    let picked = await_blocking(tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            use tauri_plugin_dialog::DialogExt;
            Ok(app
                .dialog()
                .file()
                .add_filter(filter_label, &["lic"])
                .blocking_pick_file())
        }
    }))
    .await?;

    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path.into_path().map_err(|e| CommandError {
        code: "io".into(),
        message: format!("invalid license location: {e}"),
    })?;

    let verifier = LicenseVerifier::production()?;
    let status = install_license(state.data_dir(), &path, &verifier)?;
    Ok(Some(status))
}

/// Native Open dialog for a `.oikonomia-backup` file. `None` if cancelled.
async fn pick_backup_path(
    app: &tauri::AppHandle,
    locale: Locale,
) -> CommandResult<Option<std::path::PathBuf>> {
    let filter_label = crate::tray::backup_filter_label(locale);
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
    file_path.into_path().map(Some).map_err(|e| CommandError {
        code: "io".into(),
        message: format!("invalid backup location: {e}"),
    })
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

/// Return build identity (no secrets).
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        name: "Oikonomia",
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
    let data_dir = state.data_dir().to_path_buf();
    with_vault_write_blocking(&state, move |vault| {
        let verifier = LicenseVerifier::production()?;
        let conn = vault.connection()?;
        create_entity_allowed(&data_dir, &verifier, conn, &input)
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
    with_vault_write_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        update_entity(conn, id, &name)
    })
    .await
}

/// Archive entity (soft-hide).
#[tauri::command]
pub async fn entity_archive(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    with_vault_write_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        archive_entity(conn, id)
    })
    .await
}

/// Permanently delete an entity and all of its books data.
#[tauri::command]
pub async fn entity_delete(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    with_vault_write_blocking(&state, move |vault| {
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
    with_vault_write_blocking(&state, move |vault| {
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
    with_vault_write_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        update_account(conn, &input)
    })
    .await
}

/// Archive (deactivate) account.
#[tauri::command]
pub async fn account_archive(state: State<'_, AppState>, id: AccountId) -> CommandResult<()> {
    with_vault_write_blocking(&state, move |vault| {
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
    with_vault_write_blocking(&state, move |vault| {
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
    with_vault_write_blocking(&state, move |vault| {
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
    with_vault_write_blocking(&state, move |vault| {
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

    with_vault_write_blocking(&state, move |vault| {
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
    require_writes(&state)?;
    let path_buf = std::path::PathBuf::from(&path);
    if !state.drop_path_allowed(&path_buf) {
        return Err(CommandError {
            code: "validation".into(),
            message: "file path was not dropped into the app".into(),
        });
    }

    let vault = state.vault();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let path = path_buf;
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
    with_vault_write_blocking(&state, move |vault| {
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
    with_vault_write_blocking(&state, move |vault| {
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
    with_vault_write_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        void_entry(conn, id)
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
    let path = if let Some(chosen) = input.path.clone() {
        std::path::PathBuf::from(chosen)
    } else {
        let Some(picked) = pick_csv_path(&app).await? else {
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
    with_vault_write_blocking(&state, move |vault| {
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

/// Native Open dialog for a `.csv` file. `None` if cancelled.
async fn pick_csv_path(app: &tauri::AppHandle) -> CommandResult<Option<std::path::PathBuf>> {
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
    file_path.into_path().map(Some).map_err(|e| CommandError {
        code: "io".into(),
        message: format!("invalid CSV location: {e}"),
    })
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
    with_vault_write_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        set_lock_timeout_secs(conn, secs)
    })
    .await?;
    state.set_lock_timeout_cache(secs);
    Ok(())
}

/// Get the UI theme. Plaintext preference: readable before unlock so the
/// unlock screen already renders in the user's theme.
#[tauri::command]
pub fn settings_get_theme(state: State<'_, AppState>) -> Theme {
    load_ui_prefs(state.data_dir()).theme
}

/// Persist the UI theme and sync the native window appearance. Without the
/// sync, `WKWebView` keeps drawing scrollbars and native controls in the OS
/// appearance rather than the app's theme.
#[tauri::command]
pub fn settings_set_theme(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    theme: Theme,
) -> CommandResult<()> {
    let prefs_guard = state.lock_prefs();
    let mut prefs = load_ui_prefs(state.data_dir());
    prefs.theme = theme;
    save_ui_prefs(state.data_dir(), &prefs)?;
    drop(prefs_guard);
    app.set_theme(Some(native_theme(theme)));
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

/// Full plaintext UI prefs (theme, locale, tray last-used). Safe before unlock.
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

/// Map the stored theme onto Tauri's native window theme.
pub fn native_theme(theme: Theme) -> tauri::Theme {
    match theme {
        Theme::Dark => tauri::Theme::Dark,
        Theme::Light => tauri::Theme::Light,
    }
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
    let path_buf = std::path::PathBuf::from(&path);
    if !state.drop_path_allowed(&path_buf) {
        return Err(CommandError {
            code: "validation".into(),
            message: "file path was not dropped into the app".into(),
        });
    }

    let vault = state.vault();
    let model_dir = state.ocr_model_dir().clone();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let path = path_buf;
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

fn require_writes(state: &AppState) -> CommandResult<()> {
    let verifier = LicenseVerifier::production()?;
    require_writes_allowed(state.data_dir(), &verifier).map_err(CommandError::from)
}

/// Same as [`with_vault_blocking`], but refuse the call when the trial/license
/// is expired (`license_expired`).
async fn with_vault_write_blocking<T, F>(state: &State<'_, AppState>, f: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Vault) -> Result<T, CoreError> + Send + 'static,
{
    require_writes(state)?;
    with_vault_blocking(state, f).await
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
    with_vault_write_blocking(&state, move |vault| {
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

    with_vault_write_blocking(&state, move |vault| {
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
