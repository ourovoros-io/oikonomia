//! Settings commands: the lock timeout and the plaintext preferences.

use crate::commands::support::{await_blocking, with_connection, with_vault_blocking};
use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::AppState;
use oikonomia_core::ledger::{get_lock_timeout_secs, set_lock_timeout_secs};
use oikonomia_core::prefs::{
    LastRoleAccounts, Locale, UiPrefs, last_accounts_key, load_ui_prefs, resolve_locale,
    save_ui_prefs, store_locale,
};
use tauri::{Manager, State};

/// Get auto-lock timeout seconds.
#[tauri::command]
pub(crate) async fn settings_get_lock_timeout(state: State<'_, AppState>) -> CommandResult<u64> {
    with_connection(&state, get_lock_timeout_secs).await
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
