//! Checks that the Linux appindicator library loads, before the tray needs it.
//!
//! The Linux tray (tray-icon) draws its icon through libappindicator-sys,
//! which opens libayatana-appindicator or libappindicator at run time, on
//! first use, and panics when neither loads. A panic in the Tauri setup hook
//! ends the start, so the desktop app asks this crate first and starts
//! without a tray when the answer is no
//! (<https://github.com/ourovoros-io/oikonomia/issues/206>).
//!
//! The probe tries the same names in the same order, with the same loader
//! release, so it loads exactly what libappindicator-sys would. Opening a
//! shared library is `unsafe` in libloading; this crate holds that one call,
//! so the rest of the workspace can keep `unsafe_code = "forbid"`.

use std::fmt::Display;

/// The library names libappindicator-sys 0.9.0 opens, in its order
/// (`src/lib.rs`, `LIB`). tray-icon 0.25 depends on libappindicator with its
/// default features, which turn on `backcompat`, so the names without the ABI
/// suffix are tried last.
pub const LIBRARY_NAMES: [&str; 4] = [
    "libayatana-appindicator3.so.1",
    "libappindicator3.so.1",
    "libayatana-appindicator3.so",
    "libappindicator3.so",
];

/// None of [`LIBRARY_NAMES`] could be loaded.
#[derive(Debug, thiserror::Error)]
#[error("no appindicator library loads: {}", .reasons.join("; "))]
pub struct NotFound {
    /// Why each name failed, in [`LIBRARY_NAMES`] order.
    reasons: Vec<String>,
}

/// Returns the first of [`LIBRARY_NAMES`] that `load` accepts, trying them in
/// order and stopping at the first success.
///
/// # Errors
///
/// Returns [`NotFound`], with every failure's reason, when `load` rejects
/// all of them.
pub fn first_loadable<E: Display>(
    mut load: impl FnMut(&'static str) -> Result<(), E>,
) -> Result<&'static str, NotFound> {
    let mut reasons = Vec::new();

    for name in LIBRARY_NAMES {
        match load(name) {
            Ok(()) => return Ok(name),
            Err(err) => reasons.push(err.to_string()),
        }
    }

    Err(NotFound { reasons })
}

/// Loads the appindicator library the tray will use and returns its name.
///
/// The library stays loaded for the life of the process, as it does once
/// libappindicator-sys opens it: unloading it would run its termination
/// routines, which the tray never does.
///
/// # Errors
///
/// Returns [`NotFound`] when no appindicator library can be loaded, the case
/// in which libappindicator-sys would panic.
#[cfg(target_os = "linux")]
pub fn load() -> Result<&'static str, NotFound> {
    use std::sync::OnceLock;

    static LOADED: OnceLock<libloading::Library> = OnceLock::new();

    first_loadable(|name| {
        let library = open(name)?;

        // A second call opens a library that the first one already keeps:
        // dropping this extra handle only lowers the count of `dlopen`
        // handles, and the library stays loaded.
        drop(LOADED.set(library));

        Ok::<(), libloading::Error>(())
    })
}

/// Opens the library `name` the way libappindicator-sys 0.9.0 does.
#[cfg(target_os = "linux")]
fn open(name: &str) -> Result<libloading::Library, libloading::Error> {
    #[expect(
        unsafe_code,
        reason = "opening a shared library runs its initialisation routines, \
                  which libloading cannot vouch for"
    )]
    // SAFETY: libloading 0.7.4 (`Library::new`, "# Safety") asks the caller
    // to make sure the library's initialisation routines, and its
    // termination routines on unload, are safe to run. `name` is one of
    // `LIBRARY_NAMES`, the libraries libappindicator-sys opens with this same
    // call when the tray is created, so their initialisation would run in
    // this process anyway, only later. A loaded library is kept in `load`'s
    // `LOADED` and never unloaded, so no termination routine runs.
    unsafe {
        libloading::Library::new(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_tried_in_order_until_one_loads() {
        let mut tried = Vec::new();

        let found = first_loadable(|name| {
            tried.push(name);
            if name == "libayatana-appindicator3.so" {
                Ok(())
            } else {
                Err(format!("{name}: cannot open shared object file"))
            }
        });

        assert_eq!(found.unwrap(), "libayatana-appindicator3.so");
        assert_eq!(tried, LIBRARY_NAMES[..3]);
    }

    #[test]
    fn the_first_name_that_loads_wins_and_the_rest_are_not_tried() {
        let mut tried = Vec::new();

        let found = first_loadable(|name| {
            tried.push(name);
            Ok::<(), String>(())
        });

        assert_eq!(found.unwrap(), "libayatana-appindicator3.so.1");
        assert_eq!(tried, ["libayatana-appindicator3.so.1"]);
    }

    #[test]
    fn when_no_name_loads_the_error_gives_every_reason() {
        let mut tried = Vec::new();

        let error = first_loadable(|name| {
            tried.push(name);
            Err(format!("{name}: missing"))
        })
        .unwrap_err();

        assert_eq!(tried, LIBRARY_NAMES);
        assert_eq!(
            error.to_string(),
            "no appindicator library loads: libayatana-appindicator3.so.1: missing; \
             libappindicator3.so.1: missing; libayatana-appindicator3.so: missing; \
             libappindicator3.so: missing"
        );
    }

    #[test]
    fn the_lock_file_holds_only_the_libappindicator_sys_release_the_names_come_from() {
        // `LIBRARY_NAMES` copies libappindicator-sys 0.9.0. Another release
        // in the lock file fails here, so the names are checked against its
        // source before the bump lands.
        let lock = include_str!("../../../Cargo.lock");
        let entries = lock.matches("name = \"libappindicator-sys\"\n").count();
        let pinned = lock
            .matches("name = \"libappindicator-sys\"\nversion = \"0.9.0\"\n")
            .count();

        assert_eq!(
            entries, 1,
            "Cargo.lock has {entries} libappindicator-sys entries"
        );
        assert_eq!(pinned, 1, "Cargo.lock has no libappindicator-sys 0.9.0");
    }
}
