//! Tauri command handlers (thin wrappers over core + state).

mod accounts;
mod app;
mod entities;
mod journal;
mod support;
mod vault;

// A command is a function plus a hidden macro that `generate_handler!`
// looks up beside it; a glob carries both, a named re-export does not.
pub(crate) use self::accounts::*;
pub(crate) use self::app::*;
pub(crate) use self::entities::*;
pub(crate) use self::journal::*;
pub(crate) use self::vault::*;

use crate::commands::support::{
    SaveTarget, await_blocking, decode_capped_base64, decode_document_base64, dropped_file_name,
    require_granted_path, save_with_dialog, stored_text_locale, with_vault_blocking,
};
use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::{AppState, GatedVault};
use base64::Engine;
use oikonomia_core::csv::{
    CsvImportPostInput, CsvImportPostResult, CsvImportPreview, CsvImportPreviewInput,
    default_journal_export_file_name, ensure_csv_path, export_journal_csv, post_import_rows,
    preview_bank_csv_file,
};
use oikonomia_core::documents::{
    AnalyzeContext, AnalyzerStatus, DocumentId, DocumentMeta, DocumentSuggestion,
    analyze_document_bytes, analyzer_status, attach_document, delete_document, get_document,
    list_documents, suggest_accounts_for_entity,
};
use oikonomia_core::domain::{EntityId, JournalEntryId, RecurringTemplateId};
use oikonomia_core::ledger::{
    BalanceSheet, CashFlowSeries, CreateRecurringTemplate, DashboardSummary, PnL,
    RecurringPostResult, RecurringTemplateView, TrialBalance, UpdateRecurringTemplate,
    activity_window, balance_sheet, cash_flow_series, create_recurring_template, dashboard_summary,
    delete_recurring_template, get_entity, get_lock_timeout_secs, get_recurring_template,
    list_recurring_templates, post_recurring_template, profit_and_loss, profit_and_loss_export,
    set_lock_timeout_secs, trial_balance, update_recurring_template,
};
use oikonomia_core::prefs::{
    LastRoleAccounts, Locale, UiPrefs, last_accounts_key, load_ui_prefs, resolve_locale,
    save_ui_prefs, store_locale,
};
use oikonomia_core::util::{format_date, utc_today};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{Manager, State};

// --- Recurring templates (local vault only; no auto-post) ------------------

/// List recurring templates for an entity (`due` when `next_date` ≤ UTC today).
#[tauri::command]
pub(crate) async fn recurring_list(
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
pub(crate) async fn recurring_get(
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
pub(crate) async fn recurring_create(
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
pub(crate) async fn recurring_update(
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
pub(crate) async fn recurring_delete(
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
pub(crate) async fn recurring_post(
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

// --- Reports ---------------------------------------------------------------

/// Trial balance.
#[tauri::command]
pub(crate) async fn report_trial_balance(
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
pub(crate) async fn report_pnl(
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
pub(crate) async fn report_pnl_export(
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
pub(crate) async fn report_balance_sheet(
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

/// Saves PDF bytes the webview built to a path chosen in a native Save
/// dialog. Returns the path written, or `None` if the user cancelled.
///
/// The vault is not opened, so this also works while it is locked. The call
/// counts as activity for the idle watchdog.
#[tauri::command]
pub(crate) async fn report_export_pdf(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    bytes_base64: String,
    suggested_name: Option<String>,
) -> CommandResult<Option<String>> {
    let data = decode_pdf_export_bytes(&bytes_base64)?;
    state.touch();

    let target = SaveTarget {
        filter: Some(("PDF", &["pdf"])),
        file_name: pdf_export_file_name(suggested_name.as_deref()),
        complete_path: ensure_pdf_path,
    };
    save_with_dialog(&app, target, data).await
}

/// Decoded PDF cap for a monthly expense report (webview-generated).
const MAX_PDF_EXPORT_BYTES: usize = 32 * 1024 * 1024;

fn decode_pdf_export_bytes(bytes_base64: &str) -> CommandResult<Vec<u8>> {
    decode_capped_base64(bytes_base64, MAX_PDF_EXPORT_BYTES)
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
mod pdf_export_tests {
    use super::{decode_pdf_export_bytes, ensure_pdf_path, pdf_export_file_name};
    use base64::Engine;

    #[test]
    fn decode_pdf_export_rejects_invalid_base64() {
        let err = decode_pdf_export_bytes("not-valid-base64!!!").expect_err("invalid");
        assert_eq!(err.code, "file_data_invalid");
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
pub(crate) async fn dashboard_summary_cmd(
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
pub(crate) async fn cash_flow_series_cmd(
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
pub(crate) async fn settings_get_lock_timeout(state: State<'_, AppState>) -> CommandResult<u64> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        get_lock_timeout_secs(conn)
    })
    .await
}

/// Set auto-lock timeout seconds.
#[tauri::command]
pub(crate) async fn settings_set_lock_timeout(
    state: State<'_, AppState>,
    secs: u64,
) -> CommandResult<()> {
    with_vault_blocking(&state, move |vault| {
        set_lock_timeout_secs(vault.connection()?, secs)?;

        // Under the same guard as the stored value, so that two changes
        // cannot leave the watchdog's copy and the vault disagreeing.
        vault.set_lock_timeout_cache(secs);
        Ok(())
    })
    .await
}

/// Runs preferences work on the blocking pool with the shared state.
///
/// The plaintext preferences file is read and written with blocking I/O, and
/// a save ends in an fsync. A synchronous command would do that on the main
/// thread, which also runs the event loop, and an async one on a runtime
/// worker, so every settings command goes through here.
async fn with_prefs_blocking<T, F>(app: tauri::AppHandle, work: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&tauri::AppHandle, &AppState) -> CommandResult<T> + Send + 'static,
{
    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        // `Manager::state` panics when the state is not managed, which is the
        // case after a failed start (`crate::startup`) while the hidden
        // webview is still running.
        let Some(state) = app.try_state::<AppState>() else {
            return Err(CommandError::desktop(
                DesktopError::TaskFailed,
                "application state is not set up",
            ));
        };
        work(&app, &state)
    }))
    .await
}

/// Get the native UI locale. Plaintext preference: readable before unlock so
/// tray chrome and dialogs match the user's language before a password.
#[tauri::command]
pub(crate) async fn settings_get_locale(app: tauri::AppHandle) -> CommandResult<Locale> {
    with_prefs_blocking(
        app,
        |_app, state| Ok(load_ui_prefs(state.data_dir()).locale),
    )
    .await
}

/// Persist the native UI locale, then rebuild the tray menu and refresh the
/// quick-add window title when that window exists.
#[tauri::command]
pub(crate) async fn settings_set_locale(
    app: tauri::AppHandle,
    locale: Locale,
) -> CommandResult<()> {
    with_prefs_blocking(app, move |app, state| {
        let prefs_guard = state.lock_prefs();
        store_locale(state.data_dir(), locale)?;
        drop(prefs_guard);

        crate::tray::apply_locale(app, locale);
        Ok(())
    })
    .await
}

/// The app language, chosen from the system on the very first run.
///
/// `system_languages` is the webview's report of the OS preferred languages
/// (`navigator.languages`); Rust decides everything else. When a language is
/// already stored it is returned unchanged and nothing is written, so the
/// system is consulted once per installation. Otherwise the supported
/// language is mapped, stored, and the native strings are refreshed exactly
/// as after a change in Settings. Works before a vault exists and while
/// locked, and is safe to call on every launch.
#[tauri::command]
pub(crate) async fn settings_resolve_locale(
    app: tauri::AppHandle,
    system_languages: Vec<String>,
) -> CommandResult<Locale> {
    with_prefs_blocking(app, move |app, state| {
        let prefs_guard = state.lock_prefs();
        let resolution = resolve_locale(state.data_dir(), &system_languages)?;
        drop(prefs_guard);

        // The tray was built at startup from the stored language, English on
        // a first run, so it only needs a rebuild when this call stored a new
        // one.
        if resolution.newly_stored {
            crate::tray::apply_locale(app, resolution.locale);
        }

        Ok(resolution.locale)
    })
    .await
}

/// Full plaintext UI prefs (locale, tray last-used). Safe before unlock.
#[tauri::command]
pub(crate) async fn settings_get_ui_prefs(app: tauri::AppHandle) -> CommandResult<UiPrefs> {
    with_prefs_blocking(app, |_app, state| Ok(load_ui_prefs(state.data_dir()))).await
}

/// Remember last entity + role accounts after a successful tray post.
#[tauri::command]
pub(crate) async fn settings_remember_quick_add(
    app: tauri::AppHandle,
    entity_id: String,
    kind: String,
    accounts: LastRoleAccounts,
) -> CommandResult<()> {
    with_prefs_blocking(app, move |_app, state| {
        let _prefs_guard = state.lock_prefs();

        let mut prefs = load_ui_prefs(state.data_dir());
        prefs
            .last_accounts_by_entity_kind
            .insert(last_accounts_key(&entity_id, &kind), accounts);
        prefs.last_entity_id = Some(entity_id);

        save_ui_prefs(state.data_dir(), &prefs)?;
        Ok(())
    })
    .await
}

// --- Documents / bill scan (bundled offline OCR) ---------------------------

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
/// posted ([`entry_post_simple_with_document`]). Analysis is fully offline
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
    let locale = stored_text_locale(&state);
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
        analyze_readonly(
            &vault, &model_dir, entity_id, &filename, &mime_type, &data, locale,
        )
    }))
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
    let path = require_granted_path(&state, &path)?;

    let vault = state.vault();
    let model_dir = state.ocr_model_dir().clone();
    let locale = stored_text_locale(&state);
    state.touch();

    await_blocking(tauri::async_runtime::spawn_blocking(move || {
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
    }))
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
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        list_documents(conn, entity_id)
    })
    .await
}

/// One document's bytes for the in-app viewer. Decrypted content crosses
/// IPC only; nothing is written to disk.
#[tauri::command]
pub(crate) async fn document_get(
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
pub(crate) async fn document_delete(
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

    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        attach_document(conn, entity_id, entry_id, &filename, &mime_type, &data)
    })
    .await
}

/// Export a stored document, decrypted, to a path chosen in a native Save
/// dialog. Returns the path written, or `None` if the user cancelled.
///
/// Like the journal CSV export ([`csv_export_journal`]) and the report PDF
/// ([`report_export_pdf`]), this writes plaintext to disk, and like them only
/// to a path the user picked in the dialog ([`save_with_dialog`]).
#[tauri::command]
pub(crate) async fn document_export(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    document_id: DocumentId,
) -> CommandResult<Option<String>> {
    let (meta, data) = with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        get_document(conn, document_id)
    })
    .await?;

    let target = SaveTarget {
        filter: None,
        file_name: meta.filename,
        complete_path: std::convert::identity,
    };
    save_with_dialog(&app, target, data).await
}
