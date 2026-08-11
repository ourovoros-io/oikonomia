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

use state::{AppState, resolve_ocr_model_dir};
use tauri::Manager;

/// Start the desktop application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start or the vault data dir is unusable.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // The bundled .app gets its Dock icon from icon.icns; dev mode runs
            // the bare binary, so set the icon at runtime as well.
            macos_dock_icon::set_dock_icon(include_bytes!("../icons/icon.png"));

            tray::init(app)?;

            let resource_dir = app.path().resource_dir().ok();
            let ocr_dir = resolve_ocr_model_dir(resource_dir);
            log::info!("OCR model dir: {}", ocr_dir.display());

            let app_state = AppState::new(ocr_dir).expect("failed to open vault data directory");
            let (vault, last_activity, lock_timeout) = app_state.watchdog_handles();
            app.manage(app_state);

            // Rust-side idle lock: guarantees the vault locks even if the
            // webview throttles timers or stalls entirely.
            state::spawn_auto_lock(app.handle().clone(), vault, last_activity, lock_timeout);

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
            commands::vault_init,
            commands::vault_unlock,
            commands::vault_lock,
            commands::vault_change_password,
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
            commands::entry_list,
            commands::entry_get,
            commands::entry_post,
            commands::entry_post_simple,
            commands::entry_post_simple_with_document,
            commands::entry_post_simple_with_document_path,
            commands::entry_void,
            commands::report_trial_balance,
            commands::report_pnl,
            commands::report_balance_sheet,
            commands::dashboard_summary_cmd,
            commands::settings_get_lock_timeout,
            commands::settings_set_lock_timeout,
            commands::document_analyzer_status,
            commands::document_analyze,
            commands::document_analyze_path,
            commands::document_list,
            commands::document_get,
            commands::document_delete,
            commands::document_attach,
            commands::document_export,
        ])
        .on_window_event(|window, event| {
            // Closing the window hides it to the tray instead of quitting;
            // Quit lives in the tray menu (or Cmd+Q).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();

                if let Err(err) = window.hide() {
                    log::warn!("failed to hide window on close: {err}");
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to start Oikonomia")
        .run(|app, event| {
            // Clicking the Dock icon while the window is hidden reopens it.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main_window(app);
            }

            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
