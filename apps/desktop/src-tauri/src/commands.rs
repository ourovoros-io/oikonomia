//! Tauri command handlers (thin wrappers over core + state).

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;
use base64::Engine;
use oikonomia_core::documents::{
    AnalyzerStatus, DocumentSuggestion, analyze_document_bytes, analyzer_status,
    link_document_to_entry, save_analysis_json, save_document, suggest_accounts_for_entity,
};
use oikonomia_core::domain::{Account, AccountId, Entity, EntityId, JournalEntryId};
use oikonomia_core::error::Error as CoreError;
use oikonomia_core::ledger::{
    BalanceSheet, CreateAccount, CreateEntity, DEFAULT_LOCK_TIMEOUT_SECS, DashboardSummary, PnL,
    PostJournal, PostSimpleEntry, PostedEntryView, RegisterLine, TrialBalance, UpdateAccount,
    VoidResult, account_register, archive_account, archive_entity, balance_sheet, create_account,
    create_entity, dashboard_summary, delete_entity, get_entity, get_entry, get_lock_timeout_secs,
    list_accounts, list_entities, list_entries, post_entry, post_simple_entry, profit_and_loss,
    set_lock_timeout_secs, trial_balance, update_account, update_entity, void_entry,
};
use oikonomia_core::vault::{Vault, VaultStatus};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use tauri::State;

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
#[tauri::command]
pub fn vault_status(state: State<'_, AppState>) -> CommandResult<VaultStatus> {
    state.status().map_err(Into::into)
}

/// Create a new encrypted vault with the master password.
#[tauri::command]
pub fn vault_init(state: State<'_, AppState>, password: String) -> CommandResult<VaultStatus> {
    state
        .with_vault(|vault| {
            vault.init(&password)?;
            Ok(vault.status())
        })
        .map_err(Into::into)
}

/// Unlock an existing vault.
#[tauri::command]
pub fn vault_unlock(state: State<'_, AppState>, password: String) -> CommandResult<VaultStatus> {
    let status = state.with_vault(|vault| {
        vault.unlock(&password)?;
        Ok(vault.status())
    })?;

    // Refresh the watchdog's timeout cache from the now-readable settings.
    let secs = state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            get_lock_timeout_secs(conn)
        })
        .unwrap_or(DEFAULT_LOCK_TIMEOUT_SECS);
    state.set_lock_timeout_cache(secs);

    Ok(status)
}

/// Change the master password (requires the current password).
#[tauri::command]
pub async fn vault_change_password(
    state: State<'_, AppState>,
    old: String,
    new: String,
) -> CommandResult<VaultStatus> {
    let vault = state.vault();
    state.touch();

    let joined = tauri::async_runtime::spawn_blocking(move || {
        let mut guard = vault
            .lock()
            .map_err(|_| CommandError::from(CoreError::VaultLocked))?;
        guard.change_password(&old, &new)?;
        Ok(guard.status())
    })
    .await;

    match joined {
        Ok(result) => result,
        Err(err) => Err(CommandError {
            code: "io".into(),
            message: format!("password change task failed: {err}"),
        }),
    }
}

/// Lock the vault for this session.
#[tauri::command]
pub fn vault_lock(state: State<'_, AppState>) -> CommandResult<VaultStatus> {
    state
        .with_vault(|vault| {
            vault.lock();
            Ok(vault.status())
        })
        .map_err(Into::into)
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
pub fn entity_list(state: State<'_, AppState>) -> CommandResult<Vec<Entity>> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            list_entities(conn)
        })
        .map_err(Into::into)
}

/// Create entity with chart template.
#[tauri::command]
pub fn entity_create(state: State<'_, AppState>, input: CreateEntity) -> CommandResult<Entity> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            create_entity(conn, &input)
        })
        .map_err(Into::into)
}

/// Rename entity.
#[tauri::command]
pub fn entity_update(
    state: State<'_, AppState>,
    id: EntityId,
    name: String,
) -> CommandResult<Entity> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            update_entity(conn, id, &name)
        })
        .map_err(Into::into)
}

/// Archive entity (soft-hide).
#[tauri::command]
pub fn entity_archive(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            archive_entity(conn, id)
        })
        .map_err(Into::into)
}

/// Permanently delete an entity and all of its books data.
#[tauri::command]
pub fn entity_delete(state: State<'_, AppState>, id: EntityId) -> CommandResult<()> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            delete_entity(conn, id)
        })
        .map_err(Into::into)
}

// --- Accounts --------------------------------------------------------------

/// List accounts for an entity.
#[tauri::command]
pub fn account_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
) -> CommandResult<Vec<Account>> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            list_accounts(conn, entity_id)
        })
        .map_err(Into::into)
}

/// Create account.
#[tauri::command]
pub fn account_create(state: State<'_, AppState>, input: CreateAccount) -> CommandResult<Account> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            create_account(conn, &input)
        })
        .map_err(Into::into)
}

/// Update account.
#[tauri::command]
pub fn account_update(state: State<'_, AppState>, input: UpdateAccount) -> CommandResult<Account> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            update_account(conn, &input)
        })
        .map_err(Into::into)
}

/// Archive (deactivate) account.
#[tauri::command]
pub fn account_archive(state: State<'_, AppState>, id: AccountId) -> CommandResult<()> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            archive_account(conn, id)
        })
        .map_err(Into::into)
}

/// Account register.
#[tauri::command]
pub fn account_register_cmd(
    state: State<'_, AppState>,
    account_id: AccountId,
    from: Option<String>,
    to: Option<String>,
) -> CommandResult<Vec<RegisterLine>> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            account_register(conn, account_id, from.as_deref(), to.as_deref())
        })
        .map_err(Into::into)
}

// --- Journal ---------------------------------------------------------------

/// List journal entries.
#[tauri::command]
pub fn entry_list(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: Option<String>,
    to: Option<String>,
) -> CommandResult<Vec<PostedEntryView>> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            list_entries(conn, entity_id, from.as_deref(), to.as_deref())
        })
        .map_err(Into::into)
}

/// Get one entry.
#[tauri::command]
pub fn entry_get(state: State<'_, AppState>, id: JournalEntryId) -> CommandResult<PostedEntryView> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            get_entry(conn, id)
        })
        .map_err(Into::into)
}

/// Post a balanced journal entry.
#[tauri::command]
pub fn entry_post(
    state: State<'_, AppState>,
    input: PostJournal,
) -> CommandResult<PostedEntryView> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            post_entry(conn, &input)
        })
        .map_err(Into::into)
}

/// Post a simple-form entry (kind + role accounts); line construction is in core.
#[tauri::command]
pub fn entry_post_simple(
    state: State<'_, AppState>,
    input: PostSimpleEntry,
) -> CommandResult<PostedEntryView> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            post_simple_entry(conn, &input)
        })
        .map_err(Into::into)
}

/// Void an entry (posts reverse).
#[tauri::command]
pub fn entry_void(state: State<'_, AppState>, id: JournalEntryId) -> CommandResult<VoidResult> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            void_entry(conn, id)
        })
        .map_err(Into::into)
}

// --- Reports ---------------------------------------------------------------

/// Trial balance.
#[tauri::command]
pub fn report_trial_balance(
    state: State<'_, AppState>,
    entity_id: EntityId,
    as_of: String,
) -> CommandResult<TrialBalance> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            trial_balance(conn, entity_id, &as_of)
        })
        .map_err(Into::into)
}

/// Profit and loss.
#[tauri::command]
pub fn report_pnl(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: String,
    to: String,
) -> CommandResult<PnL> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            profit_and_loss(conn, entity_id, &from, &to)
        })
        .map_err(Into::into)
}

/// Balance sheet.
#[tauri::command]
pub fn report_balance_sheet(
    state: State<'_, AppState>,
    entity_id: EntityId,
    as_of: String,
) -> CommandResult<BalanceSheet> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            balance_sheet(conn, entity_id, &as_of)
        })
        .map_err(Into::into)
}

/// Dashboard summary.
#[tauri::command]
pub fn dashboard_summary_cmd(
    state: State<'_, AppState>,
    entity_id: EntityId,
    from: String,
    to: String,
) -> CommandResult<DashboardSummary> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            dashboard_summary(conn, entity_id, &from, &to)
        })
        .map_err(Into::into)
}

// --- Settings --------------------------------------------------------------

/// Get auto-lock timeout seconds.
#[tauri::command]
pub fn settings_get_lock_timeout(state: State<'_, AppState>) -> CommandResult<u64> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            get_lock_timeout_secs(conn)
        })
        .map_err(Into::into)
}

/// Set auto-lock timeout seconds.
#[tauri::command]
pub fn settings_set_lock_timeout(state: State<'_, AppState>, secs: u64) -> CommandResult<()> {
    state.with_vault(|vault| {
        let conn = vault.connection()?;
        set_lock_timeout_secs(conn, secs)
    })?;
    state.set_lock_timeout_cache(secs);
    Ok(())
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
        analyze_with_vault(&vault, &model_dir, entity_id, &filename, &mime_type, &data)
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
    let vault = state.vault();
    let model_dir = state.ocr_model_dir().clone();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let path = std::path::PathBuf::from(&path);
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
        analyze_with_vault(&vault, &model_dir, entity_id, &filename, &mime, &data)
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

/// Store, analyze, and annotate a document.
///
/// The vault lock is held only while touching the database; the CPU-heavy
/// OCR/parsing phase runs unlocked so other commands stay responsive (F8).
fn analyze_with_vault(
    vault: &Mutex<Vault>,
    model_dir: &Path,
    entity_id: EntityId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
) -> CommandResult<DocumentSuggestion> {
    let (meta, accounts, entity) = {
        let guard = vault
            .lock()
            .map_err(|_| CommandError::from(CoreError::VaultLocked))?;
        let conn = guard.connection()?;
        let entity = get_entity(conn, entity_id)?;
        let meta = save_document(conn, entity_id, filename, mime_type, data)?;
        let accounts = suggest_accounts_for_entity(conn, entity_id)?;
        (meta, accounts, entity)
    };

    let suggestion = analyze_document_bytes(
        meta.id,
        &meta.filename,
        &meta.mime_type,
        data,
        &accounts,
        &entity.base_currency,
        Some(model_dir),
    )?;

    if let Ok(json) = serde_json::to_string(&suggestion) {
        if let Ok(guard) = vault.lock() {
            if let Ok(conn) = guard.connection() {
                let _ = save_analysis_json(conn, meta.id, &json);
            }
        }
    }

    Ok(suggestion)
}

/// Link a stored document to a journal entry after the user posts.
#[tauri::command]
pub fn document_link_entry(
    state: State<'_, AppState>,
    document_id: oikonomia_core::documents::DocumentId,
    entry_id: JournalEntryId,
) -> CommandResult<()> {
    state
        .with_vault(|vault| {
            let conn = vault.connection()?;
            link_document_to_entry(conn, document_id, entry_id)
        })
        .map_err(Into::into)
}
