//! System tray: left-click quick-add; menu Open / Quit; close hides windows.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl};

const QUICK_ADD_LABEL: &str = "quick-add";
/// Match web rolling panel (`QUICK_ADD_WIDTH` / `QUICK_ADD_IDLE_HEIGHT` in TS).
const QUICK_ADD_WIDTH: f64 = 360.0;
const QUICK_ADD_HEIGHT: f64 = 132.0;

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
    if let Some(window) = app.get_webview_window(QUICK_ADD_LABEL)
        && let Err(err) = window.hide()
    {
        log::warn!("failed to hide quick-add window: {err}");
    }
}

fn ensure_quick_add_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(existing) = app.get_webview_window(QUICK_ADD_LABEL) {
        return Ok(existing);
    }

    // Transparent + undecorated so the web UI can draw rounded corners
    // (native chrome is rectangular).
    let window =
        WebviewWindowBuilder::new(app, QUICK_ADD_LABEL, WebviewUrl::App("index.html".into()))
            .title("Quick add")
            .inner_size(QUICK_ADD_WIDTH, QUICK_ADD_HEIGHT)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .build()?;

    Ok(window)
}

/// Position near tray click when possible; otherwise top-right of primary monitor work area is fine.
fn position_quick_add(window: &tauri::WebviewWindow, click: Option<PhysicalPosition<f64>>) {
    let Ok(outer) = window.outer_size() else {
        return;
    };
    let width = f64::from(outer.width);
    let height = f64::from(outer.height);

    let (x, y) = if let Some(pos) = click {
        (pos.x - width / 2.0, pos.y - height - 8.0)
    } else {
        (40.0, 40.0)
    };

    // Screen coords are whole pixels after round; i32 is what Tauri expects.
    #[expect(clippy::cast_possible_truncation)]
    let pos = PhysicalPosition {
        x: x.round() as i32,
        y: y.round().max(0.0) as i32,
    };
    let _ = window.set_position(tauri::Position::Physical(pos));
}

/// Create (if needed), position, show, and focus the quick-add panel.
pub fn show_quick_add(app: &AppHandle, click: Option<PhysicalPosition<f64>>) {
    match ensure_quick_add_window(app) {
        Ok(window) => {
            position_quick_add(&window, click);
            if let Err(err) = window.show() {
                log::warn!("failed to show quick-add: {err}");
            }
            if let Err(err) = window.set_focus() {
                log::warn!("failed to focus quick-add: {err}");
            }
        }
        Err(err) => log::error!("failed to create quick-add window: {err}"),
    }
}

/// Build the tray icon with left-click quick-add and Open / Quit menu.
pub fn init(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Oikonomia", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Oikonomia", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Oikonomia")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                show_quick_add(tray.app_handle(), Some(position));
            }
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
