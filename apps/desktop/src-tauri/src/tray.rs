//! The system tray: its icon, its menu, and the quick-add window it opens.
//!
//! # Left click and menu
//!
//! A left click on the tray icon opens the quick-add window: a small,
//! always-on-top companion for posting one entry without opening the main
//! window. The tray menu, which the platform shows on a right click, offers
//! "Open Oikonomia" and "Quit Oikonomia". Closing the main window only hides
//! it (`on_window_event` in `lib.rs`), so the menu is how it comes back and,
//! with the system's own quit shortcut, how the app exits.
//!
//! # Linux
//!
//! Linux trays (libayatana-appindicator) report no click on the icon; they
//! only ever show the menu. There the menu gets a "Quick add" entry, first in
//! the list ([`TRAY_REPORTS_CLICKS`]). No click position is known on that
//! path, so the window goes to the top-right corner of the work area, where
//! Linux desktops keep their tray.
//!
//! # Placement
//!
//! Where a click position and the monitor's work area are known (Windows),
//! the window is centred on the click, on the side of it that has room, and
//! pulled inside the work area ([`quick_add_origin`]). macOS reports no work
//! area on purpose and keeps the placement that was tuned on real hardware.
//!
//! # Language
//!
//! The native strings that never pass through the webview are worded here,
//! one function per string: the menu entries, the quick-add window's title
//! and the backup dialog's file filter. They follow the stored language, and
//! [`apply_locale`] rewords the live tray after a change.

use oikonomia_core::prefs::{Locale, load_ui_prefs};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl};

/// Whether this platform's tray reports clicks on the icon. Linux
/// (libayatana-appindicator) only ever shows the menu.
const TRAY_REPORTS_CLICKS: bool = !cfg!(target_os = "linux");

/// The window label of the quick-add window. The frontend tells the two
/// windows apart by it.
const QUICK_ADD_LABEL: &str = "quick-add";

// Sizes of the quick-add window in logical pixels. The frontend resizes the
// window between them at runtime with its own copies, the `QUICK_ADD_*`
// constants in `web/src/lib/quickAddWindow.ts`; the test
// `quick_add_sizes_match_the_web_constants` fails when the two sets differ.

/// Width in every state.
const QUICK_ADD_WIDTH: f64 = 300.0;
/// Height the window is created with (the stepper row).
const QUICK_ADD_STEPPER_HEIGHT: f64 = 64.0;
/// Height of the one-line states (locked, saved, no books).
const QUICK_ADD_COMPACT_HEIGHT: f64 = 56.0;
/// The tallest the frontend makes the window (the confirm row).
const QUICK_ADD_SAVE_HEIGHT: f64 = 96.0;

// `placement_height` makes room for the save row on the ground that no state
// is taller.
const _: () = assert!(
    QUICK_ADD_SAVE_HEIGHT >= QUICK_ADD_STEPPER_HEIGHT
        && QUICK_ADD_SAVE_HEIGHT >= QUICK_ADD_COMPACT_HEIGHT,
    "the save row must be the tallest quick-add state"
);

/// Gap between the quick-add window and the tray click or screen edge.
const QUICK_ADD_GAP: f64 = 8.0;

/// Returns the tray menu entry that opens the main window.
#[must_use]
pub(crate) fn tray_open_label(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "Open Oikonomia",
        Locale::El => "Άνοιγμα Oikonomia",
        Locale::Fr => "Ouvrir Oikonomia",
        Locale::De => "Oikonomia öffnen",
    }
}

/// Returns the tray menu entry that quits the app.
#[must_use]
pub(crate) fn tray_quit_label(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "Quit Oikonomia",
        Locale::El => "Έξοδος από το Oikonomia",
        Locale::Fr => "Quitter Oikonomia",
        Locale::De => "Oikonomia beenden",
    }
}

/// Returns the title of the quick-add window, which is also the Linux menu
/// entry that opens it.
#[must_use]
pub(crate) fn quick_add_title(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "Quick add",
        Locale::El => "Γρήγορη καταχώριση",
        Locale::Fr => "Saisie rapide",
        Locale::De => "Schnellerfassung",
    }
}

/// Returns the file-type label the native dialogs show for
/// `.oikonomia-backup` archives.
#[must_use]
pub(crate) fn backup_filter_label(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "Oikonomia backup",
        Locale::El => "Αντίγραφο ασφαλείας Oikonomia",
        Locale::Fr => "Sauvegarde Oikonomia",
        Locale::De => "Oikonomia-Sicherung",
    }
}

/// Returns the tray icon's tooltip: the brand name, the same in every
/// language.
#[must_use]
pub(crate) fn tray_tooltip(_locale: Locale) -> &'static str {
    "Oikonomia"
}

/// One entry of the tray menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayMenuItem {
    /// Opens the quick-add window. Listed only where the icon reports no click.
    QuickAdd,
    /// Shows the main window.
    Open,
    /// Exits the app.
    Quit,
}

impl TrayMenuItem {
    /// Returns the entries in display order. "Quick add" is listed only where a
    /// click on the icon cannot open the quick-add window.
    fn for_tray(reports_clicks: bool) -> &'static [Self] {
        if reports_clicks {
            &[Self::Open, Self::Quit]
        } else {
            &[Self::QuickAdd, Self::Open, Self::Quit]
        }
    }

    /// Returns the identifier the entry is registered under and its menu event
    /// carries.
    const fn id(self) -> &'static str {
        match self {
            Self::QuickAdd => "quick_add",
            Self::Open => "open",
            Self::Quit => "quit",
        }
    }

    /// Returns the entry a menu event's identifier names, if it is one of ours.
    fn from_id(id: &str) -> Option<Self> {
        [Self::QuickAdd, Self::Open, Self::Quit]
            .into_iter()
            .find(|item| item.id() == id)
    }

    /// Returns the entry's text in `locale`.
    fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::QuickAdd => quick_add_title(locale),
            Self::Open => tray_open_label(locale),
            Self::Quit => tray_quit_label(locale),
        }
    }
}

/// Builds the tray menu for this platform, worded in `locale`.
///
/// # Errors
///
/// Returns Tauri's error when a menu or an entry cannot be created.
fn build_menu<R: tauri::Runtime, M: Manager<R>>(app: &M, locale: Locale) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;

    for item in TrayMenuItem::for_tray(TRAY_REPORTS_CLICKS) {
        let entry = MenuItem::with_id(app, item.id(), item.label(locale), true, None::<&str>)?;
        menu.append(&entry)?;
    }

    Ok(menu)
}

/// Rewords the tray menu and the quick-add window's title after a language
/// change.
///
/// A failure is logged and otherwise ignored: the language itself is already
/// stored, and stale native text is not worth failing the command for.
pub(crate) fn apply_locale(app: &AppHandle, locale: Locale) {
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

/// Brings the main window back after it was hidden to the tray: shows it,
/// restores it if minimized, and focuses it.
///
/// Does nothing after a failed start, when there is no state behind the
/// window.
pub(crate) fn show_main_window(app: &AppHandle) {
    // No state means the start failed: the window is hidden behind the
    // failure message (`crate::startup`), and none of its commands would work.
    if app.try_state::<crate::state::AppState>().is_none() {
        return;
    }

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

/// Hides the quick-add window. Does nothing when the window does not exist.
pub(crate) fn hide_quick_add(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(QUICK_ADD_LABEL)
        && let Err(err) = window.hide()
    {
        log::warn!("failed to hide quick-add window: {err}");
    }
}

/// Returns the quick-add window, creating it hidden on first use.
///
/// # Errors
///
/// Returns Tauri's error when the window cannot be created.
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

/// Returns the stored language, or the default when the app has no state.
fn locale_from_app(app: &AppHandle) -> Locale {
    app.try_state::<crate::state::AppState>()
        .map(|state| load_ui_prefs(state.data_dir()).locale)
        .unwrap_or_default()
}

/// The part of a monitor not covered by the taskbar or panels, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct WorkArea {
    /// The left edge.
    x: f64,
    /// The top edge.
    y: f64,
    /// The width.
    width: f64,
    /// The height.
    height: f64,
}

impl WorkArea {
    /// Returns the right edge.
    fn right(self) -> f64 {
        self.x + self.width
    }

    /// Returns the bottom edge.
    fn bottom(self) -> f64 {
        self.y + self.height
    }
}

/// Returns the top-left corner for a quick-add window of `size`, in physical
/// pixels.
///
/// With a tray click and a work area, the window is centred on the click and
/// sits on the side of it that has room: above a click in the lower half of
/// the screen (a bottom taskbar), below one in the upper half (a top panel).
/// It is then pulled inside the work area, so a tray icon in a screen corner
/// cannot push it off screen.
///
/// Without a click (the Linux menu entry) it goes to the top-right corner of
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

/// Returns `start` moved so that a span of `length` lies inside `low..high`
/// with [`QUICK_ADD_GAP`] left at each end. A span longer than the range
/// starts at `low`.
fn keep_inside(start: f64, length: f64, low: f64, high: f64) -> f64 {
    let lowest = low + QUICK_ADD_GAP;
    let highest = high - length - QUICK_ADD_GAP;

    if highest < lowest {
        low
    } else {
        start.clamp(lowest, highest)
    }
}

/// Returns the work area of the monitor under `click`, or of the primary
/// monitor.
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

/// Returns the height to place the quick-add window by, in physical pixels:
/// its current height, or its tallest state when it is being fitted to a
/// work area.
fn placement_height(current: f64, scale: f64, fitted_to_work_area: bool) -> f64 {
    if fitted_to_work_area {
        current.max(QUICK_ADD_SAVE_HEIGHT * scale)
    } else {
        current
    }
}

/// Moves the quick-add window next to the tray click and onto the screen.
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

    #[expect(
        clippy::cast_possible_truncation,
        reason = "a rounded screen coordinate; Tauri positions are whole i32 pixels"
    )]
    let position = PhysicalPosition {
        x: x.round() as i32,
        y: y.round() as i32,
    };
    let _ = window.set_position(tauri::Position::Physical(position));
}

/// Shows the quick-add window: creates it if needed, positions it, shows it
/// and gives it focus.
///
/// `click` is where the tray icon was clicked, or `None` when the window is
/// opened from the Linux menu entry. A failure is logged; there is no caller
/// that could act on it.
pub(crate) fn show_quick_add(app: &AppHandle, click: Option<PhysicalPosition<f64>>) {
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

/// Creates the tray icon, with its menu and its click handler.
///
/// The menu is worded in `locale`. A left click opens the quick-add window
/// and never the menu.
///
/// # Errors
///
/// Returns Tauri's error when the menu, the icon image or the tray itself
/// cannot be created. The caller fails startup.
pub(crate) fn init(app: &tauri::App, locale: Locale) -> tauri::Result<()> {
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

    /// Returns the number in the web source's `export const <name> = <number>`
    /// line.
    fn web_constant(source: &str, name: &str) -> f64 {
        let declaration = format!("export const {name} = ");
        let value = source
            .lines()
            .find_map(|line| line.strip_prefix(declaration.as_str()))
            .expect("the web source declares the constant");

        value.trim().parse().expect("the constant is a number")
    }

    #[test]
    fn quick_add_sizes_match_the_web_constants() {
        let web = include_str!("../../../../web/src/lib/quickAddWindow.ts");
        let native = [
            ("QUICK_ADD_WIDTH", QUICK_ADD_WIDTH),
            ("QUICK_ADD_STEPPER_HEIGHT", QUICK_ADD_STEPPER_HEIGHT),
            ("QUICK_ADD_COMPACT_HEIGHT", QUICK_ADD_COMPACT_HEIGHT),
            ("QUICK_ADD_SAVE_HEIGHT", QUICK_ADD_SAVE_HEIGHT),
        ];

        for (name, value) in native {
            assert_eq!(web_constant(web, name), value, "{name}");
        }
    }

    /// A 1920 by 1080 monitor with a 40 pixel taskbar along the bottom.
    const FULL_HD: WorkArea = WorkArea {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1040.0,
    };
    /// The quick-add window at its opening size, as width and height.
    const PANEL: (f64, f64) = (300.0, 64.0);

    /// Fails unless a window of `size` at `origin` lies wholly inside `area`.
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
