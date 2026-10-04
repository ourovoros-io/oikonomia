//! System tray: left-click quick-add; menu Open / Quit; close hides windows.
//!
//! Linux trays deliver no click events, only the menu, so there the menu
//! carries a "Quick add" item instead.

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
/// The tallest the frontend makes the window (the confirm row).
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

/// Whether this platform's tray reports clicks on the icon. Linux
/// (libayatana-appindicator) only ever shows the menu.
const TRAY_REPORTS_CLICKS: bool = !cfg!(target_os = "linux");

/// One entry of the tray menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayMenuItem {
    QuickAdd,
    Open,
    Quit,
}

impl TrayMenuItem {
    /// Entries in display order. "Quick add" is listed only where a click on
    /// the icon cannot open the quick-add window.
    fn for_tray(reports_clicks: bool) -> &'static [Self] {
        if reports_clicks {
            &[Self::Open, Self::Quit]
        } else {
            &[Self::QuickAdd, Self::Open, Self::Quit]
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::QuickAdd => "quick_add",
            Self::Open => "open",
            Self::Quit => "quit",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        [Self::QuickAdd, Self::Open, Self::Quit]
            .into_iter()
            .find(|item| item.id() == id)
    }

    fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::QuickAdd => quick_add_title(locale),
            Self::Open => tray_open_label(locale),
            Self::Quit => tray_quit_label(locale),
        }
    }
}

fn build_menu<R: tauri::Runtime, M: Manager<R>>(app: &M, locale: Locale) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;

    for item in TrayMenuItem::for_tray(TRAY_REPORTS_CLICKS) {
        let entry = MenuItem::with_id(app, item.id(), item.label(locale), true, None::<&str>)?;
        menu.append(&entry)?;
    }

    Ok(menu)
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

    // A minimized window is neither hidden nor focusable; without this a
    // relaunch on Windows would appear to do nothing.
    if let Err(err) = window.unminimize() {
        log::warn!("failed to restore main window: {err}");
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

/// Gap between the quick-add window and the tray click or screen edge.
const QUICK_ADD_GAP: f64 = 8.0;

/// The part of a monitor not covered by the taskbar or panels, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct WorkArea {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl WorkArea {
    fn right(self) -> f64 {
        self.x + self.width
    }

    fn bottom(self) -> f64 {
        self.y + self.height
    }
}

/// Top-left corner for a quick-add window of `size`, in physical pixels.
///
/// With a tray click and a work area, the window is centred on the click and
/// sits on the side of it that has room: above a click in the lower half of
/// the screen (a bottom taskbar), below one in the upper half (a top panel).
/// It is then pulled inside the work area, so a tray icon in a screen corner
/// cannot push it off screen.
///
/// Without a click (the Linux menu item) it goes to the top-right corner of
/// the work area, where Linux desktops keep their tray. Without a work area
/// it is centred above the click and kept at or below the top of the screen;
/// macOS uses that path and applies its own menu-bar constraint.
fn quick_add_origin(
    click: Option<(f64, f64)>,
    size: (f64, f64),
    work_area: Option<WorkArea>,
) -> (f64, f64) {
    let (width, height) = size;

    let Some(area) = work_area else {
        return match click {
            Some((x, y)) => (x - width / 2.0, (y - height - QUICK_ADD_GAP).max(0.0)),
            None => (40.0, 40.0),
        };
    };

    let (x, y) = match click {
        Some((x, y)) => {
            let middle = area.y + area.height / 2.0;
            let y = if y > middle {
                y - height - QUICK_ADD_GAP
            } else {
                y + QUICK_ADD_GAP
            };
            (x - width / 2.0, y)
        }
        None => (area.right() - width, area.y),
    };

    (
        keep_inside(x, width, area.x, area.right()),
        keep_inside(y, height, area.y, area.bottom()),
    )
}

/// Moves a span of `length` starting at `start` inside `low..high`, leaving
/// [`QUICK_ADD_GAP`] at each end. A span longer than the range starts at `low`.
fn keep_inside(start: f64, length: f64, low: f64, high: f64) -> f64 {
    let lowest = low + QUICK_ADD_GAP;
    let highest = high - length - QUICK_ADD_GAP;

    if highest < lowest {
        low
    } else {
        start.clamp(lowest, highest)
    }
}

/// Work area of the monitor under `click`, or of the primary monitor.
///
/// macOS reports none on purpose: its placement was tuned on real hardware
/// and the system keeps the window under the menu bar.
fn work_area_for(app: &AppHandle, click: Option<(f64, f64)>) -> Option<WorkArea> {
    if cfg!(target_os = "macos") {
        return None;
    }

    let monitor = click
        .and_then(|(x, y)| app.monitor_from_point(x, y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let area = monitor.work_area();

    Some(WorkArea {
        x: f64::from(area.position.x),
        y: f64::from(area.position.y),
        width: f64::from(area.size.width),
        height: f64::from(area.size.height),
    })
}

/// Height to place the quick-add window by, in physical pixels: its current
/// height, or its tallest state when it is being fitted to a work area.
fn placement_height(current: f64, scale: f64, fitted_to_work_area: bool) -> f64 {
    if fitted_to_work_area {
        current.max(QUICK_ADD_SAVE_HEIGHT * scale)
    } else {
        current
    }
}

/// Place the quick-add window next to the tray click, on screen.
fn position_quick_add(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    click: Option<PhysicalPosition<f64>>,
) {
    let Ok(outer) = window.outer_size() else {
        return;
    };
    let click = click.map(|position| (position.x, position.y));
    let work_area = work_area_for(app, click);

    // The frontend grows the window downwards to its confirm row without
    // moving it. Where the window is fitted to a work area, make room for
    // that height now, or the confirm row would end up under the taskbar.
    let scale = window.scale_factor().unwrap_or(1.0);
    let height = placement_height(f64::from(outer.height), scale, work_area.is_some());

    let (x, y) = quick_add_origin(click, (f64::from(outer.width), height), work_area);

    // Screen coords are whole pixels after round; i32 is what Tauri expects.
    #[expect(clippy::cast_possible_truncation)]
    let pos = PhysicalPosition {
        x: x.round() as i32,
        y: y.round() as i32,
    };
    let _ = window.set_position(tauri::Position::Physical(pos));
}

/// Create (if needed), position, show, and focus the quick-add panel.
pub fn show_quick_add(app: &AppHandle, click: Option<PhysicalPosition<f64>>) {
    match ensure_quick_add_window(app) {
        Ok(window) => {
            position_quick_add(app, &window, click);
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
        .on_menu_event(
            |app, event| match TrayMenuItem::from_id(event.id.as_ref()) {
                Some(TrayMenuItem::QuickAdd) => show_quick_add(app, None),
                Some(TrayMenuItem::Open) => show_main_window(app),
                Some(TrayMenuItem::Quit) => app.exit(0),
                None => {}
            },
        )
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

    const FULL_HD: WorkArea = WorkArea {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1040.0,
    };
    const PANEL: (f64, f64) = (300.0, 64.0);

    fn assert_inside(origin: (f64, f64), size: (f64, f64), area: WorkArea) {
        let (x, y) = origin;
        assert!(
            x >= area.x && x + size.0 <= area.right(),
            "x {x} off screen"
        );
        assert!(
            y >= area.y && y + size.1 <= area.bottom(),
            "y {y} off screen"
        );
    }

    #[test]
    fn tray_without_click_events_offers_quick_add_in_the_menu() {
        assert_eq!(
            TrayMenuItem::for_tray(false),
            [
                TrayMenuItem::QuickAdd,
                TrayMenuItem::Open,
                TrayMenuItem::Quit
            ]
        );
        assert_eq!(
            TrayMenuItem::for_tray(true),
            [TrayMenuItem::Open, TrayMenuItem::Quit]
        );
    }

    #[test]
    fn quick_add_is_placed_by_its_tallest_state_so_it_clears_the_taskbar() {
        // The window opens 64 high and the frontend grows it to 96 without
        // moving it. Placed by its opening height it would end under a
        // bottom taskbar once grown.
        let opening = QUICK_ADD_STEPPER_HEIGHT;
        let height = placement_height(opening, 1.0, true);
        assert_eq!(height, QUICK_ADD_SAVE_HEIGHT);

        let origin = quick_add_origin(Some((1500.0, 1060.0)), (PANEL.0, height), Some(FULL_HD));
        assert!(origin.1 + QUICK_ADD_SAVE_HEIGHT <= FULL_HD.bottom());

        let by_opening_height =
            quick_add_origin(Some((1500.0, 1060.0)), (PANEL.0, opening), Some(FULL_HD));
        assert!(by_opening_height.1 + QUICK_ADD_SAVE_HEIGHT > FULL_HD.bottom());
    }

    #[test]
    fn placement_height_scales_with_the_display_and_is_unchanged_without_a_work_area() {
        assert_eq!(placement_height(128.0, 2.0, true), 192.0);
        assert_eq!(placement_height(240.0, 2.0, true), 240.0);
        assert_eq!(placement_height(64.0, 2.0, false), 64.0);
    }

    #[test]
    fn every_menu_item_round_trips_through_its_id_and_has_a_label() {
        for item in TrayMenuItem::for_tray(false) {
            assert_eq!(TrayMenuItem::from_id(item.id()), Some(*item));
            for locale in [Locale::En, Locale::El, Locale::Fr, Locale::De] {
                assert_ne!(item.label(locale), "");
            }
        }
        assert_eq!(TrayMenuItem::from_id("unknown"), None);
        assert_eq!(TrayMenuItem::QuickAdd.label(Locale::En), "Quick add");
    }

    #[test]
    fn quick_add_opens_above_a_click_on_a_bottom_taskbar() {
        // Windows: the taskbar is below the work area, tray near the right.
        let origin = quick_add_origin(Some((1500.0, 1060.0)), PANEL, Some(FULL_HD));

        assert_eq!(origin.0, 1350.0);
        assert_eq!(origin.1, FULL_HD.bottom() - PANEL.1 - QUICK_ADD_GAP);
        assert_inside(origin, PANEL, FULL_HD);
    }

    #[test]
    fn quick_add_opens_below_a_click_on_a_top_panel() {
        let below_panel = WorkArea { y: 32.0, ..FULL_HD };

        let origin = quick_add_origin(Some((1500.0, 16.0)), PANEL, Some(below_panel));

        assert_eq!(origin, (1350.0, 32.0 + QUICK_ADD_GAP));
        assert_inside(origin, PANEL, below_panel);
    }

    #[test]
    fn quick_add_stays_on_screen_when_the_tray_is_in_a_corner() {
        let corners = [(1915.0, 1070.0), (2.0, 1070.0), (1915.0, 4.0), (2.0, 4.0)];

        for click in corners {
            let origin = quick_add_origin(Some(click), PANEL, Some(FULL_HD));
            assert_inside(origin, PANEL, FULL_HD);
        }
    }

    #[test]
    fn quick_add_uses_the_monitor_the_click_is_on() {
        // A second monitor to the left has negative coordinates.
        let left_monitor = WorkArea {
            x: -1280.0,
            y: 0.0,
            width: 1280.0,
            height: 984.0,
        };

        let origin = quick_add_origin(Some((-20.0, 1000.0)), PANEL, Some(left_monitor));

        assert_inside(origin, PANEL, left_monitor);
    }

    #[test]
    fn quick_add_from_the_menu_goes_to_the_top_right_of_the_work_area() {
        let origin = quick_add_origin(None, PANEL, Some(FULL_HD));

        assert_eq!(
            origin,
            (
                FULL_HD.right() - PANEL.0 - QUICK_ADD_GAP,
                FULL_HD.y + QUICK_ADD_GAP
            )
        );
    }

    #[test]
    fn quick_add_larger_than_the_work_area_starts_at_its_origin() {
        let tiny = WorkArea {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 40.0,
        };

        assert_eq!(quick_add_origin(None, PANEL, Some(tiny)), (10.0, 20.0));
    }

    #[test]
    fn quick_add_without_a_work_area_keeps_the_original_placement() {
        assert_eq!(
            quick_add_origin(Some((800.0, 12.0)), PANEL, None),
            (650.0, 0.0)
        );
        assert_eq!(
            quick_add_origin(Some((800.0, 900.0)), PANEL, None),
            (650.0, 900.0 - PANEL.1 - QUICK_ADD_GAP)
        );
        assert_eq!(quick_add_origin(None, PANEL, None), (40.0, 40.0));
    }
}
