//! Tauri application entry: thin IPC over `oikonomia-core`.

// Tauri commands take `State<'_, T>` by value (framework convention).
// Shell startup uses expect/process::exit via the Tauri runtime.
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::expect_used)]
#![allow(clippy::exit)]

mod commands;
mod error;
mod state;
mod tray;
mod update;
mod update_exec;
mod update_key;

use state::{AppState, resolve_ocr_model_dir};
use tauri::Manager;

/// Start the desktop application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start or the vault data dir is unusable.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    with_desktop_plugins(tauri::Builder::default())
        .setup(|app| {
            // The bundled .app gets its Dock icon from icon.icns; dev mode runs
            // the bare binary, so set the icon at runtime as well.
            macos_dock_icon::set_dock_icon(include_bytes!("../icons/icon.png"));

            let resource_dir = app.path().resource_dir().ok();
            let ocr_dir = resolve_ocr_model_dir(resource_dir);
            log::info!("OCR model dir: {}", ocr_dir.display());

            let app_state = AppState::new(ocr_dir).expect("failed to open vault data directory");
            let watchdog = app_state.watchdog_handles();

            // Native window appearance (scrollbars, controls, title bar) must
            // match the stored theme, not the OS preference.
            let prefs = oikonomia_core::prefs::load_ui_prefs(app_state.data_dir());
            app.handle()
                .set_theme(Some(commands::native_theme(prefs.theme)));

            tray::init(app, prefs.locale)?;

            app.manage(app_state);

            // Rust-side idle lock: guarantees the vault locks even if the
            // webview throttles timers or stalls entirely.
            state::spawn_auto_lock(app.handle().clone(), watchdog);

            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault_status,
            commands::vault_touch,
            commands::vault_init,
            commands::vault_unlock,
            commands::license_status,
            commands::license_install,
            update::update_check,
            update::update_install,
            commands::vault_lock,
            commands::vault_change_password,
            commands::vault_backup,
            commands::vault_restore,
            commands::vault_pick_backup,
            commands::app_info,
            commands::entity_list,
            commands::entity_create,
            commands::entity_update,
            commands::entity_archive,
            commands::entity_delete,
            commands::account_list,
            commands::account_create,
            commands::account_update,
            commands::account_archive,
            commands::account_register_cmd,
            commands::account_balance_cmd,
            commands::account_set_opening_balance,
            commands::entry_list,
            commands::entry_replace_simple,
            commands::entry_get,
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
            commands::settings_get_lock_timeout,
            commands::settings_set_lock_timeout,
            commands::settings_get_theme,
            commands::settings_set_theme,
            commands::settings_get_locale,
            commands::settings_set_locale,
            commands::settings_get_ui_prefs,
            commands::settings_remember_quick_add,
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
        ])
        .on_window_event(on_window_event)
        .build(tauri::generate_context!())
        .expect("failed to start Oikonomia")
        .run(on_run_event);
}

fn on_run_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    // Clicking the Dock icon while the window is hidden reopens it.
    #[cfg(target_os = "macos")]
    if let tauri::RunEvent::Reopen { .. } = event {
        tray::show_main_window(app);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, event);
}

/// Dialog, window-state, and updater install engine. No `check()` here.
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
        // Registered only. Install execs the wrapper-verified path; no check API.
        .plugin(tauri_plugin_updater::Builder::new().build())
}

fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    // Closing the window hides it to the tray instead of quitting;
    // Quit lives in the tray menu (or Cmd+Q).
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();

        if let Err(err) = window.hide() {
            log::warn!("failed to hide window on close: {err}");
        }
    }

    record_native_drops(window, event);
}

fn record_native_drops(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event
        && let Some(state) = window.try_state::<AppState>()
    {
        state.remember_drop_paths(paths.iter().cloned());
    }
}
