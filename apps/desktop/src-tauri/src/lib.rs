//! Tauri application entry: thin IPC over `oikonomia-core`.

// Tauri commands take `State<'_, T>` by value (framework convention).
// Shell startup uses expect/process::exit via the Tauri runtime.
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::expect_used)]
#![allow(clippy::exit)]

mod commands;
mod error;
mod state;

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
        .setup(|app| {
            let resource_dir = app.path().resource_dir().ok();
            let ocr_dir = resolve_ocr_model_dir(resource_dir);
            log::info!("OCR model dir: {}", ocr_dir.display());

            let app_state =
                AppState::new(ocr_dir).expect("failed to open vault data directory");
            app.manage(app_state);

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
            commands::document_link_entry,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Oikonomia");
}
