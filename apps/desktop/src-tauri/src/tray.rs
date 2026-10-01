//! System tray: left-click quick-add; menu Open / Quit; close hides windows.

use oikonomia_core::prefs::{Locale, load_ui_prefs};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl};

const QUICK_ADD_LABEL: &str = "quick-add";
/// Match web one-row rolling tray (`QUICK_ADD_*` in quickAddWindow.ts).
const QUICK_ADD_WIDTH: f64 = 300.0;
const QUICK_ADD_STEPPER_HEIGHT: f64 = 64.0;
/// Kept in lockstep with web; FE resizes to these at runtime.
#[expect(dead_code)]
const QUICK_ADD_COMPACT_HEIGHT: f64 = 56.0;
#[expect(dead_code)]
const QUICK_ADD_SAVE_HEIGHT: f64 = 96.0;

/// Tray menu: open the main window.
#[must_use]
pub fn tray_open_label(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "Open Oikonomia",
        Locale::El => "Άνοιγμα Oikonomia",
        Locale::Fr => "Ouvrir Oikonomia",
        Locale::De => "Oikonomia öffnen",
    }
}

/// Tray menu: quit the app.
#[must_use]
pub fn tray_quit_label(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "Quit Oikonomia",
        Locale::El => "Έξοδος από το Oikonomia",
        Locale::Fr => "Quitter Oikonomia",
        Locale::De => "Oikonomia beenden",
    }
}

/// Quick-add companion window title.
#[must_use]
pub fn quick_add_title(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "Quick add",
        Locale::El => "Γρήγορη καταχώριση",
        Locale::Fr => "Saisie rapide",
        Locale::De => "Schnellerfassung",
    }
}

/// Native file-dialog filter for `.oikonomia-backup` archives.
#[must_use]
pub fn backup_filter_label(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "Oikonomia backup",
        Locale::El => "Αντίγραφο ασφαλείας Oikonomia",
        Locale::Fr => "Sauvegarde Oikonomia",
        Locale::De => "Oikonomia-Sicherung",
    }
}

/// Tray tooltip is the brand name in every locale.
#[must_use]
pub fn tray_tooltip(_locale: Locale) -> &'static str {
    "Oikonomia"
}

fn locale_from_app(app: &AppHandle) -> Locale {
    app.try_state::<crate::state::AppState>()
        .map(|state| load_ui_prefs(state.data_dir()).locale)
        .unwrap_or_default()
}

fn build_menu<R: tauri::Runtime, M: Manager<R>>(app: &M, locale: Locale) -> tauri::Result<Menu<R>> {
    let open = MenuItem::with_id(app, "open", tray_open_label(locale), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tray_quit_label(locale), true, None::<&str>)?;
    Menu::with_items(app, &[&open, &quit])
}

/// Rebuild the tray menu and refresh the quick-add title after a locale change.
pub fn apply_locale(app: &AppHandle, locale: Locale) {
    match build_menu(app, locale) {
        Ok(menu) => {
            if let Some(tray) = app.tray_by_id("main")
                && let Err(err) = tray.set_menu(Some(menu))
            {
                log::warn!("failed to update tray menu: {err}");
            }
        }
        Err(err) => log::warn!("failed to rebuild tray menu: {err}"),
    }

    if let Some(window) = app.get_webview_window(QUICK_ADD_LABEL)
        && let Err(err) = window.set_title(quick_add_title(locale))
    {
        log::warn!("failed to set quick-add title: {err}");
    }
}

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
            .title(quick_add_title(locale_from_app(app)))
            .inner_size(QUICK_ADD_WIDTH, QUICK_ADD_STEPPER_HEIGHT)
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

/// Position near tray click (one-row tray-anchored). Fallback: top-left of primary work area.
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
pub fn init(app: &tauri::App, locale: Locale) -> tauri::Result<()> {
    let menu = build_menu(app, locale)?;

    let tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip(tray_tooltip(locale))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_labels_switch_en_el_fr_de() {
        assert_eq!(tray_open_label(Locale::En), "Open Oikonomia");
        assert_eq!(tray_open_label(Locale::El), "Άνοιγμα Oikonomia");
        assert_eq!(tray_open_label(Locale::Fr), "Ouvrir Oikonomia");
        assert_eq!(tray_open_label(Locale::De), "Oikonomia öffnen");
        assert_eq!(tray_quit_label(Locale::En), "Quit Oikonomia");
        assert_eq!(tray_quit_label(Locale::El), "Έξοδος από το Oikonomia");
        assert_eq!(tray_quit_label(Locale::Fr), "Quitter Oikonomia");
        assert_eq!(tray_quit_label(Locale::De), "Oikonomia beenden");
        assert_eq!(quick_add_title(Locale::En), "Quick add");
        assert_eq!(quick_add_title(Locale::El), "Γρήγορη καταχώριση");
        assert_eq!(quick_add_title(Locale::Fr), "Saisie rapide");
        assert_eq!(quick_add_title(Locale::De), "Schnellerfassung");
        assert_eq!(backup_filter_label(Locale::En), "Oikonomia backup");
        assert_eq!(
            backup_filter_label(Locale::El),
            "Αντίγραφο ασφαλείας Oikonomia"
        );
        assert_eq!(backup_filter_label(Locale::Fr), "Sauvegarde Oikonomia");
        assert_eq!(backup_filter_label(Locale::De), "Oikonomia-Sicherung");
        assert_eq!(tray_tooltip(Locale::En), "Oikonomia");
        assert_eq!(tray_tooltip(Locale::El), "Oikonomia");
        assert_eq!(tray_tooltip(Locale::Fr), "Oikonomia");
        assert_eq!(tray_tooltip(Locale::De), "Oikonomia");
    }
}
