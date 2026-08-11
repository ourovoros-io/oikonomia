//! System tray: closing the window hides it; the tray menu restores or quits.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

/// Bring the main window back after it was hidden to the tray.
pub fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    if let Err(err) = window.show() {
        log::warn!("failed to show main window: {err}");
    }

    if let Err(err) = window.set_focus() {
        log::warn!("failed to focus main window: {err}");
    }
}

/// Hide the tray quick-add window if it exists (no-op when missing).
pub fn hide_quick_add(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("quick-add") {
        if let Err(err) = window.hide() {
            log::warn!("failed to hide quick-add window: {err}");
        }
    }
}

/// Build the tray icon with its Open / Quit menu.
pub fn init(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Oikonomia", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Oikonomia", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Oikonomia")
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        });

    // The macOS menu bar wants a monochrome template image (only the alpha
    // channel is used); other platforms reuse the colored window icon.
    let tray = if cfg!(target_os = "macos") {
        tray.icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
            .icon_as_template(true)
    } else if let Some(icon) = app.default_window_icon().cloned() {
        tray.icon(icon)
    } else {
        tray
    };

    tray.build(app)?;

    Ok(())
}
