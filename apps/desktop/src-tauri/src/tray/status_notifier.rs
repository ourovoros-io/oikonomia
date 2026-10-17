//! Whether the Linux desktop shows tray icons right now.
//!
//! The Linux tray (libayatana-appindicator) hands its icon to the
//! `StatusNotifier` watcher on the session bus, and the icon is shown only when
//! a `StatusNotifier` host, a panel that draws such icons, has registered with
//! that watcher. Stock GNOME has no host, and a session with no session bus
//! has no watcher either. The watcher answers both questions through its
//! `IsStatusNotifierHostRegistered` property; when no process owns the
//! watcher's name the call fails, which counts as no host.
//!
//! The question is asked each time the answer is needed, not once at start,
//! so a panel that starts after the app counts.
//!
//! An `XEmbed` tray, which libayatana-appindicator falls back to when there is
//! no watcher, is not detected, so such a desktop counts as having no tray.
//! The caller quits rather than hides the main window then, and a relaunch
//! recovers from that, while a hidden window with no tray would not be.

use std::time::Duration;
use zbus::zvariant::OwnedValue;

/// The watcher's well-known name on the session bus, which is also the
/// interface that carries the property.
const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";

/// The object path the watcher serves.
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

/// The watcher's property that says whether a host has registered.
const HOST_REGISTERED_PROPERTY: &str = "IsStatusNotifierHostRegistered";

/// How long the watcher may take to answer. The question is asked on the
/// main thread when the user closes the window, so a stalled bus must not
/// freeze it for long.
const ANSWER_TIMEOUT: Duration = Duration::from_millis(500);

/// Returns whether a `StatusNotifier` host is registered on the session bus,
/// that is, whether a tray icon would be shown. Any failure to find out,
/// a missing session bus included, counts as no host.
pub(crate) fn host_registered() -> bool {
    match ask_watcher() {
        Ok(registered) => registered,
        Err(err) => {
            log::info!("no StatusNotifier host on the session bus: {err}");
            false
        }
    }
}

/// Asks the `StatusNotifier` watcher whether a host has registered.
///
/// # Errors
///
/// Returns zbus's error when there is no session bus, no process owns the
/// watcher's name, the watcher does not answer in [`ANSWER_TIMEOUT`], or the
/// property is not a boolean.
fn ask_watcher() -> zbus::Result<bool> {
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(ANSWER_TIMEOUT)
        .build()?;

    let reply = connection.call_method(
        Some(WATCHER_NAME),
        WATCHER_PATH,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &(WATCHER_NAME, HOST_REGISTERED_PROPERTY),
    )?;

    let value: OwnedValue = reply.body().deserialize()?;
    Ok(bool::try_from(value)?)
}
