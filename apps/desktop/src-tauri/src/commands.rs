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
    BalanceSheet, CreateAccount, CreateEntity, DEFAULT_LOCK_TIMEOUT_SECS, DashboardSummary,
    EntryFilter, PnL, PostJournal, PostSimpleEntry, PostedEntryView, RegisterLine, TrialBalance,
    UpdateAccount, VoidResult, account_register, archive_account, archive_entity, balance_sheet,
    create_account, create_entity, dashboard_summary, delete_entity, get_entity, get_entry,
    get_lock_timeout_secs, list_accounts, list_entities, list_entries, post_entry,
    post_simple_entry, profit_and_loss, set_lock_timeout_secs, trial_balance, update_account,
    update_entity, void_entry,
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
///
/// Async so the frontend's activity heartbeat never blocks the main thread,
/// even while a long operation (rekey, analysis save) holds the vault mutex.
#[tauri::command]
pub async fn vault_status(state: State<'_, AppState>) -> CommandResult<VaultStatus> {
    let vault = state.vault();
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let guard = crate::state::lock_vault(&vault);
        Ok(guard.status())
    }))
    .await
}

/// Create a new encrypted vault with the master password.
#[tauri::command]
pub async fn vault_init(
    state: State<'_, AppState>,
    password: String,
) -> CommandResult<VaultStatus> {
    with_vault_blocking(&state, move |vault| {
        vault.init(&password)?;
        Ok(vault.status())
    })
    .await
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

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        let mut guard = crate::state::lock_vault(&vault);
        guard.change_password(&old, &new)?;
        Ok(guard.status())
    }))
    .await
}

/// Lock the vault for this session.
#[tauri::command]
pub async fn vault_lock(state: State<'_, AppState>) -> CommandResult<VaultStatus> {
    with_vault_blocking(&state, move |vault| {
        vault.lock();
        Ok(vault.status())
    })
    .await
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
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        set_lock_timeout_secs(conn, secs)
    })
    .await?;
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
        let guard = crate::state::lock_vault(vault);
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

    // Persist strictly: if the vault locked mid-analysis (watchdog or user),
    // surface the error instead of silently dropping the analysis snapshot.
    {
        let guard = crate::state::lock_vault(vault);
        let conn = guard.connection()?;
        let json = serde_json::to_string(&suggestion).map_err(|e| CommandError {
            code: "io".into(),
            message: format!("could not serialize analysis: {e}"),
        })?;
        save_analysis_json(conn, meta.id, &json)?;
    }

    Ok(suggestion)
}

/// Link a stored document to a journal entry after the user posts.
#[tauri::command]
pub async fn document_link_entry(
    state: State<'_, AppState>,
    document_id: oikonomia_core::documents::DocumentId,
    entry_id: JournalEntryId,
) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        link_document_to_entry(conn, document_id, entry_id)
    })
    .await
}
