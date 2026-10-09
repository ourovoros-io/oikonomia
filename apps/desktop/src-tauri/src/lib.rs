//! The Tauri desktop shell: the windows, the tray and the IPC layer over
//! `oikonomia-core`.
//!
//! The shell owns what only a desktop process can do: the windows and the
//! tray (`tray`), native dialogs, the idle watchdog thread (`state`),
//! installing an update (`update`, `update_exec`), and the commands the
//! webview invokes (`commands`). It holds no ledger rules. Those are in core,
//! and a command is a thin wrapper over a core call.
//!
//! # What the shell guarantees
//!
//! - The webview reaches the backend only through the commands listed in
//!   `ipc_commands`, and cannot navigate away from the app's own origin
//!   (`nav_guard`).
//! - A path named by the webview is used only if the user handed it over in
//!   a native drop or a native dialog, and only for what it was handed over
//!   for (`state::PathGrants`, `state::GrantPurpose`).
//! - The vault locks after the idle timeout whatever the webview is doing
//!   (`state::spawn_auto_lock`).
//! - An update is installed the way the running copy was installed, and
//!   never over a copy the system package manager owns (`update_exec`).
//! - An error crosses IPC as a code plus parameters; the UI never shows text
//!   written here (`error`).
//! - A release build logs warnings and errors only, never ledger data, to a
//!   size-capped local file. The webview reaches it through one command only,
//!   which takes a fixed location and a filtered message
//!   (`commands::frontend_log`).
//! - One process per user: a second launch on Windows or Linux shows the
//!   running app's window and exits (`with_single_instance`).

mod commands;
#[cfg(test)]
mod config_checks;
mod donations;
mod error;
mod error_log;
mod nav_guard;
mod startup;
mod state;
mod tray;
mod update;
mod update_exec;
mod update_key;

use startup::StartupError;
use state::{AppState, GrantPurpose, resolve_ocr_model_dir};
use tauri::Manager;

/// Starts the desktop application and runs it until it exits.
///
/// A start that fails once the runtime is up (a damaged vault header, an
/// unreadable data directory) is reported in a native message and ends with a
/// failure exit code (the `startup` module).
///
/// # Panics
///
/// Panics if the Tauri runtime itself cannot be built.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = with_desktop_plugins(
        with_single_instance(tauri::Builder::default()).plugin(nav_guard::plugin()),
    )
    .setup(|app| {
        // Never an `Err` from here: Tauri panics on one. A failed start is
        // shown to the user instead, and the app exits when they dismiss it.
        if let Err(failure) = start(app) {
            startup::report_and_exit(app, &failure);
        }
        Ok(())
    })
    .invoke_handler(ipc_commands())
    .on_window_event(on_window_event);

    #[expect(
        clippy::expect_used,
        reason = "without a runtime there is no window or dialog to report through, \
                  so the panic message is the report"
    )]
    let app = builder
        .build(tauri::generate_context!())
        .expect("failed to start Oikonomia");

    app.run(on_run_event);
}

/// Sets up the logger, the state, the tray and the idle watchdog, in that
/// order.
///
/// The state is handed to Tauri last, after the watchdog has started, so no
/// command can reach the vault without the watchdog running.
///
/// # Errors
///
/// Returns the first step's failure as a [`StartupError`]; the caller shows
/// it to the user and exits.
fn start(app: &mut tauri::App) -> Result<(), StartupError> {
    // The bundled .app gets its Dock icon from icon.icns; dev mode runs
    // the bare binary, so set the icon at runtime as well.
    macos_dock_icon::set_dock_icon(include_bytes!("../icons/icon.png"));

    // First, so that the log lines of the steps below reach it.
    register_logger(app)?;

    let resource_dir = app.path().resource_dir().ok();
    let ocr_dir = resolve_ocr_model_dir(resource_dir);
    log::info!("OCR model dir: {}", ocr_dir.display());

    let app_state = startup::open_app_state(ocr_dir)?;
    let watchdog = app_state.watchdog_handles();

    // Native window appearance (scrollbars, controls, title bar) must
    // match the app, not the OS preference. The UI is dark-only, so a
    // user who once picked the retired light theme still gets dark
    // native chrome instead of a light title bar around a dark window.
    app.handle().set_theme(Some(tauri::Theme::Dark));

    // The tray menu is worded in the stored language; the webview applies
    // its own copy of the preference later.
    let prefs = oikonomia_core::prefs::load_ui_prefs(app_state.data_dir());
    tray::init(app, prefs.locale()).map_err(StartupError::Shell)?;

    // Rust-side idle lock: guarantees the vault locks even if the
    // webview throttles timers or stalls entirely. Started before the state
    // is managed, so no command can reach the vault without it running.
    state::spawn_auto_lock(app.handle().clone(), watchdog).map_err(StartupError::Watchdog)?;

    app.manage(app_state);

    // Records this start's version, for the notice after an update. No
    // network: the updater only connects when the user asks it to.
    app.manage(update::UpdaterState::at_start(
        app.path().app_config_dir().ok(),
    ));

    Ok(())
}

/// Registers the logger: the developer's in a debug build, the local error
/// log in a release build.
///
/// A debug build registers `tauri-plugin-log` at the info level, with the
/// plugin's default targets (standard output and a file in the app's log
/// directory), and asks core to write errors in full
/// (`oikonomia_core::error::enable_log_detail`).
///
/// A release build does neither. It installs [`error_log`], which writes the
/// warnings and errors of this workspace's crates to one size-capped,
/// owner-only file in the app's log directory, so that a failed start or a
/// failed update install leaves a record on the machine it happened on. What
/// may be in that file is a privacy decision, and the `error_log` module
/// states it: no ledger data, and nothing the webview sends. The plugin is
/// not registered there, so its `log` command does not exist in a release
/// build.
///
/// # Errors
///
/// Returns [`StartupError::Shell`] when the plugin cannot be registered in a
/// debug build. A release build returns no error: without a log directory, or
/// with one that cannot be written, the app starts without a log.
fn register_logger(app: &tauri::App) -> Result<(), StartupError> {
    if cfg!(debug_assertions) {
        oikonomia_core::error::enable_log_detail();

        return app
            .handle()
            .plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )
            .map_err(StartupError::Shell);
    }

    // No log is not a reason to refuse to start, and without a log there is
    // nowhere to say why there is none.
    if let Ok(directory) = app.path().app_log_dir() {
        error_log::install(&directory).ok();
    }
    Ok(())
}

/// Handles an event of the app's run loop: on macOS, a click on the Dock
/// icon while the window is hidden brings the window back.
// Only macOS inspects the event without consuming it; elsewhere it is moved
// into the unused-arguments tuple and the lint has nothing to report.
#[cfg_attr(
    target_os = "macos",
    expect(
        clippy::needless_pass_by_value,
        reason = "the signature `tauri::App::run` calls back with"
    )
)]
fn on_run_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    // Clicking the Dock icon while the window is hidden reopens it.
    #[cfg(target_os = "macos")]
    if let tauri::RunEvent::Reopen { .. } = event {
        tray::show_main_window(app);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, event);
}

/// Returns `builder` with the plugin that makes a second launch show the
/// running app's main window and exit.
///
/// Registered before every other plugin so the second process stops before
/// it creates a window or touches the vault directory. Without it, closing
/// the window to the tray on a desktop that shows no tray (stock GNOME) would
/// leave the app running with no way back in, and a relaunch would open the
/// same vault from two processes.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn with_single_instance(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.plugin(tauri_plugin_single_instance::init(
        |app, _arguments, _working_directory| tray::show_main_window(app),
    ))
}

/// Returns `builder` unchanged: macOS routes a second launch to the running
/// app itself (`RunEvent::Reopen`).
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn with_single_instance(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
}

/// Returns `builder` with the dialog, window-state and opener plugins.
///
/// There is no updater plugin: updates are checked by `oikonomia-update` and
/// installed by `update_exec`.
fn with_desktop_plugins(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        .plugin(tauri_plugin_dialog::init())
        // Remember window size/position across launches. VISIBLE is excluded:
        // quitting from the tray while hidden must not restore an invisible
        // window on the next start.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        & !tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        // Used from Rust only (open_support_email); the webview holds no opener permission.
        .plugin(tauri_plugin_opener::init())
}

/// Returns the handler for every IPC command the webview may invoke; nothing
/// else is reachable.
///
/// The names here are the names the frontend invokes, so the list is part of
/// the contract with it.
fn ipc_commands() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::vault_status,
        commands::vault_touch,
        commands::vault_init,
        commands::vault_unlock,
        update::update_check,
        update::update_install,
        update::update_cancel,
        update::update_take_notice,
        commands::vault_lock,
        commands::vault_change_password,
        commands::vault_backup,
        commands::vault_restore,
        commands::vault_pick_backup,
        commands::app_info,
        commands::open_support_email,
        commands::log_frontend_error,
        donations::donation_addresses,
        commands::entity_list,
        commands::entity_list_archived,
        commands::entity_create,
        commands::entity_update,
        commands::entity_archive,
        commands::entity_unarchive,
        commands::entity_delete,
        commands::account_list,
        commands::account_defaults,
        commands::account_create,
        commands::account_update,
        commands::account_archive,
        commands::account_register_cmd,
        commands::account_balance_cmd,
        commands::account_set_opening_balance,
        commands::entry_list,
        commands::entry_replace_simple,
        commands::entry_get,
        commands::entry_history,
        commands::entry_post,
        commands::entry_post_simple,
        commands::entry_post_simple_with_document,
        commands::entry_post_simple_with_document_path,
        commands::entry_void,
        commands::entry_set_hidden,
        commands::recurring_list,
        commands::recurring_get,
        commands::recurring_create,
        commands::recurring_update,
        commands::recurring_delete,
        commands::recurring_post,
        commands::csv_import_preview,
        commands::csv_import_post,
        commands::csv_export_journal,
        commands::report_trial_balance,
        commands::report_pnl,
        commands::report_pnl_export,
        commands::report_balance_sheet,
        commands::report_export_pdf,
        commands::dashboard_summary_cmd,
        commands::cash_flow_series_cmd,
        commands::settings_get_lock_timeout,
        commands::settings_set_lock_timeout,
        commands::settings_get_locale,
        commands::settings_set_locale,
        commands::settings_resolve_locale,
        commands::settings_get_ui_prefs,
        commands::settings_reset_ui_prefs,
        commands::settings_remember_quick_add,
        commands::settings_remember_last_entity,
        commands::open_main_window,
        commands::quick_add_hide,
        commands::document_analyzer_status,
        commands::document_analyze,
        commands::document_analyze_path,
        commands::document_list,
        commands::document_get,
        commands::document_delete,
        commands::document_attach,
        commands::document_export,
    ]
}

/// What a close request does to a window instead of closing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseAction {
    /// Hide the window; the tray menu or a relaunch brings it back.
    HideToTray,
    /// Minimize the window, which the taskbar or the window switcher restores.
    Minimize,
}

impl CloseAction {
    /// Returns what closing the window labelled `label` does.
    ///
    /// A Linux desktop can show no tray at all (stock GNOME has no
    /// `AppIndicator` host) and the app cannot tell, so a hidden main window
    /// could be left running with nothing to bring it back or to quit it.
    /// There the main window is minimized, which every desktop can undo.
    fn for_window(label: &str, on_linux: bool) -> Self {
        if on_linux && label == "main" {
            Self::Minimize
        } else {
            Self::HideToTray
        }
    }
}

/// Handles a window event: a close request keeps the app running (see
/// [`CloseAction`]), and a file drop is recorded as a grant.
///
/// Closing does not quit; Quit is in the tray menu and on the system's quit
/// shortcut.
fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    // Closing the window keeps the app running instead of quitting;
    // Quit lives in the tray menu (or Cmd+Q).
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();

        let action = CloseAction::for_window(window.label(), cfg!(target_os = "linux"));
        let kept = match action {
            CloseAction::HideToTray => window.hide(),
            CloseAction::Minimize => window.minimize(),
        };
        if let Err(err) = kept {
            log::warn!("failed to {action:?} the window on close: {err}");
        }
    }

    record_native_drops(window, event);
}

/// Grants the paths of files the user dropped on a window as documents, so
/// that the two commands that read a dropped document accept them. A drop
/// grants nothing else: a dropped file is not accepted as a backup to
/// restore or as a statement to import.
///
/// Does nothing before the state exists.
fn record_native_drops(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event
        && let Some(state) = window.try_state::<AppState>()
    {
        state.grant_paths(GrantPurpose::Document, paths.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_main_window_minimizes_it_on_linux_and_hides_it_elsewhere() {
        assert_eq!(CloseAction::for_window("main", true), CloseAction::Minimize);
        assert_eq!(
            CloseAction::for_window("main", false),
            CloseAction::HideToTray
        );
    }

    #[test]
    fn closing_the_quick_add_window_hides_it_everywhere() {
        // It cannot be minimized, and it has no taskbar entry to restore it.
        assert_eq!(
            CloseAction::for_window("quick-add", true),
            CloseAction::HideToTray
        );
        assert_eq!(
            CloseAction::for_window("quick-add", false),
            CloseAction::HideToTray
        );
    }
}
