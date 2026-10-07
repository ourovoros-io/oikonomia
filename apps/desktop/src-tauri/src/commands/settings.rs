//! Settings commands: the idle lock timeout and the plaintext preferences.
//!
//! The two kinds of setting live in different places. The lock timeout is
//! stored inside the vault, so its commands require the unlocked vault. The
//! language and the quick-add window's last choices are stored in a plaintext
//! preferences file beside the vault, so that the tray and the unlock screen
//! can use them before a password is entered; their commands work in every
//! vault state and go through [`with_prefs_blocking`].

use crate::commands::support::{run_blocking, with_connection, with_vault_blocking};
use crate::error::{CommandError, CommandResult, DesktopError};
use crate::state::AppState;
use oikonomia_core::domain::EntityId;
use oikonomia_core::ledger::{SimpleEntryKind, get_lock_timeout_secs, set_lock_timeout_secs};
use oikonomia_core::prefs::{
    LastRoleAccounts, Locale, UiPrefs, last_accounts_key, load_ui_prefs, resolve_locale,
    save_ui_prefs, store_locale,
};
use tauri::{Manager, State};

/// Returns the idle time, in seconds, after which the vault locks itself.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns the [common vault errors](crate::commands#common-vault-errors).
#[tauri::command]
pub(crate) async fn settings_get_lock_timeout(state: State<'_, AppState>) -> CommandResult<u64> {
    with_connection(&state, get_lock_timeout_secs).await
}

/// Stores the idle time, in seconds, after which the vault locks itself, and
/// hands the new value to the idle watchdog.
///
/// Requires the unlocked vault.
///
/// # Errors
///
/// Returns `lock_timeout_too_short` (with the minimum as `min_secs`) for a value
/// under the minimum, and the
/// [common vault errors](crate::commands#common-vault-errors).
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

/// Returns the stored app language.
///
/// Works in every vault state: the preference is plaintext so that the tray
/// and native dialogs are in the user's language before a password is
/// entered. A missing or unreadable preferences file yields the default
/// language.
///
/// # Errors
///
/// Returns `task_failed` when the application state was never set up or the
/// blocking task panics.
#[tauri::command]
pub(crate) async fn settings_get_locale(app: tauri::AppHandle) -> CommandResult<Locale> {
    with_prefs_blocking(
        app,
        |_app, state| Ok(load_ui_prefs(state.data_dir()).locale),
    )
    .await
}

/// Stores the app language, then rebuilds the tray menu and retitles the
/// quick-add window in it.
///
/// Works in every vault state.
///
/// # Errors
///
/// Returns `io` when the preferences file cannot be written, and
/// `task_failed` when the application state was never set up or the blocking
/// task panics.
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

/// Returns the app language, choosing it from the system's languages on the
/// very first run.
///
/// `system_languages` is the webview's report of the system's preferred
/// languages (`navigator.languages`); Rust decides everything else. When a
/// language is already stored it is returned unchanged and nothing is
/// written, so the system is consulted once per installation. Otherwise the
/// first supported language is stored and the native strings are refreshed
/// exactly as after a change in Settings. Works in every vault state and
/// before a vault exists, and is safe to call on every launch.
///
/// # Errors
///
/// Returns `io` when the preferences file cannot be written, and
/// `task_failed` when the application state was never set up or the blocking
/// task panics.
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

/// Returns the whole plaintext preferences: the language and what the
/// quick-add window last used.
///
/// Works in every vault state. A missing or unreadable preferences file
/// yields the defaults.
///
/// # Errors
///
/// Returns `task_failed` when the application state was never set up or the
/// blocking task panics.
#[tauri::command]
pub(crate) async fn settings_get_ui_prefs(app: tauri::AppHandle) -> CommandResult<UiPrefs> {
    with_prefs_blocking(app, |_app, state| Ok(load_ui_prefs(state.data_dir()))).await
}

/// Remembers the entity, and the accounts chosen for an entry kind, that the
/// quick-add window last posted with.
///
/// Works in every vault state. The values go to the plaintext preferences
/// file; they are identifiers, with no amount and no name among them.
///
/// # Errors
///
/// Returns `io` when the preferences file cannot be written, and
/// `task_failed` when the application state was never set up or the blocking
/// task panics.
#[tauri::command]
pub(crate) async fn settings_remember_quick_add(
    app: tauri::AppHandle,
    entity_id: EntityId,
    kind: SimpleEntryKind,
    accounts: LastRoleAccounts,
) -> CommandResult<()> {
    with_prefs_blocking(app, move |_app, state| {
        let _prefs_guard = state.lock_prefs();

        let mut prefs = load_ui_prefs(state.data_dir());
        prefs
            .last_accounts_by_entity_kind
            .insert(last_accounts_key(entity_id, kind), accounts);
        prefs.last_entity_id = Some(entity_id.to_string());

        save_ui_prefs(state.data_dir(), &prefs)?;
        Ok(())
    })
    .await
}

/// Runs preferences work on the blocking pool with the shared state.
///
/// The plaintext preferences file is read and written with blocking I/O, and
/// a save ends in an fsync. A synchronous command would do that on the main
/// thread, which also runs the event loop, and an async one on a runtime
/// worker, so every preferences command goes through here.
///
/// # Errors
///
/// Returns the error `work` returns, and `task_failed` when the application
/// state was never set up or the blocking task panics.
async fn with_prefs_blocking<T, F>(app: tauri::AppHandle, work: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&tauri::AppHandle, &AppState) -> CommandResult<T> + Send + 'static,
{
    run_blocking(move || {
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
    })
    .await
}
