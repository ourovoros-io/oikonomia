//! Whether the Linux desktop shows tray icons right now.
//!
//! The Linux tray (libayatana-appindicator) hands its icon to the
//! `StatusNotifier` watcher on the session bus, and the icon is shown only
//! when a `StatusNotifier` host, a panel that draws such icons, has
//! registered with that watcher. Stock GNOME has no host, and a session with
//! no session bus has no watcher either. The bus says whether the watcher's
//! name has an owner, and the watcher's `IsStatusNotifierHostRegistered`
//! property says whether a host has registered. The name is checked first so
//! that the bus does not try to start a watcher just to be asked.
//!
//! The question is asked each time the answer is needed, not once at start,
//! so a panel that starts after the app counts.
//!
//! An `XEmbed` tray, which libayatana-appindicator falls back to when there
//! is no watcher, is not detected, so such a desktop counts as having no
//! tray. The caller quits rather than hides the main window then, and a
//! relaunch recovers from that, while a hidden window with no tray would not
//! be.

use std::sync::mpsc;
use std::time::Duration;
use zbus::blocking::Connection;
use zbus::zvariant::OwnedValue;

/// The message bus itself, which knows which names have an owner.
const BUS_NAME: &str = "org.freedesktop.DBus";

/// The object path the message bus serves.
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// The watcher's well-known name on the session bus, which is also the
/// interface that carries the property.
const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";

/// The object path the watcher serves.
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

/// The watcher's property that says whether a host has registered.
const HOST_REGISTERED_PROPERTY: &str = "IsStatusNotifierHostRegistered";

/// How long the whole question may take, connecting to the bus included. It
/// is asked on the main thread when the user closes the window.
const ANSWER_TIMEOUT: Duration = Duration::from_millis(500);

/// Returns whether a `StatusNotifier` host is registered on the session bus,
/// that is, whether a tray icon would be shown.
///
/// Waits at most [`ANSWER_TIMEOUT`]. Any failure to find out, a missing
/// session bus or a bus that does not answer in time included, counts as no
/// host.
pub(crate) fn host_registered() -> bool {
    let (sender, receiver) = mpsc::sync_channel(1);

    // The connection itself has no timeout in zbus (`method_timeout` covers
    // method calls only), so the question runs on its own thread, and a bus
    // that stalls leaves that thread behind rather than the window.
    let spawned = std::thread::Builder::new()
        .name("tray-host-check".into())
        .spawn(move || sender.send(ask_bus()).ok());
    if let Err(err) = spawned {
        log::warn!("cannot ask the session bus for a tray host: {err}");
        return false;
    }

    match receiver.recv_timeout(ANSWER_TIMEOUT) {
        Ok(Ok(registered)) => registered,
        Ok(Err(err)) => {
            log::info!("no StatusNotifier host on the session bus: {err}");
            false
        }
        Err(_) => {
            log::warn!("the session bus did not say in {ANSWER_TIMEOUT:?} whether a tray is shown");
            false
        }
    }
}

/// Asks the session bus whether the watcher runs and, if it does, whether a
/// host has registered with it.
///
/// # Errors
///
/// Returns zbus's error when there is no session bus, a call fails, or an
/// answer is not a boolean.
fn ask_bus() -> zbus::Result<bool> {
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(ANSWER_TIMEOUT)
        .build()?;

    if !watcher_has_owner(&connection)? {
        return Ok(false);
    }

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

/// Returns whether a process owns the watcher's name on `connection`'s bus.
///
/// # Errors
///
/// Returns zbus's error when the bus does not answer.
fn watcher_has_owner(connection: &Connection) -> zbus::Result<bool> {
    let reply = connection.call_method(
        Some(BUS_NAME),
        BUS_PATH,
        Some(BUS_NAME),
        "NameHasOwner",
        &(WATCHER_NAME,),
    )?;

    reply.body().deserialize()
}
