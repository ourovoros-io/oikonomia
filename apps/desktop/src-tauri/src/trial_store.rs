//! Secondary trial stamp. Deleting the app-data dir must not reset the trial.
//!
//! `oikonomia_core::license` keeps the canonical, always-present stamp in
//! `ui-prefs.json`. This module supplies the platform's best *secondary*
//! location for a mirror of that stamp, per
//! [`oikonomia_core::license::TrialStampStore`]: on macOS, a generic
//! password in the login Keychain; on every other desktop target, nothing
//! (the trial behaves exactly as it did before this module existed).

use oikonomia_core::license::TrialStampStore;

/// Keychain generic password: service = bundle identifier, account = stamp.
#[cfg(target_os = "macos")]
pub struct MacKeychainTrialStore;

#[cfg(target_os = "macos")]
const SERVICE: &str = "io.ourovoros.oikonomia";
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "trial-started-at";

#[cfg(target_os = "macos")]
impl TrialStampStore for MacKeychainTrialStore {
    fn read_stamp(&self) -> Option<String> {
        security_framework::passwords::get_generic_password(SERVICE, ACCOUNT)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    fn write_stamp(&self, rfc3339: &str) {
        if let Err(err) = security_framework::passwords::set_generic_password(
            SERVICE,
            ACCOUNT,
            rfc3339.as_bytes(),
        ) {
            log::warn!("keychain trial stamp write failed: {err}");
        }
    }
}

/// The platform's best secondary stamp store.
#[must_use]
pub fn default_trial_store() -> &'static dyn TrialStampStore {
    #[cfg(target_os = "macos")]
    {
        static STORE: MacKeychainTrialStore = MacKeychainTrialStore;
        &STORE
    }
    #[cfg(not(target_os = "macos"))]
    {
        static STORE: oikonomia_core::license::NoTrialStampStore =
            oikonomia_core::license::NoTrialStampStore;
        &STORE
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// Round-trips a stamp through the real login Keychain and deletes it.
    ///
    /// Not run by default: `cargo test -p oikonomia --ignored keychain`
    /// touches the developer's actual login keychain (an OS prompt may
    /// appear the first time). CI and `cargo test --workspace` never hit it.
    #[test]
    #[ignore = "touches the real login keychain"]
    fn keychain_round_trips_and_deletes_a_stamp() {
        let store = MacKeychainTrialStore;
        let stamp = "2026-01-01T00:00:00Z";

        store.write_stamp(stamp);
        assert_eq!(store.read_stamp().as_deref(), Some(stamp));

        security_framework::passwords::delete_generic_password(SERVICE, ACCOUNT)
            .expect("delete the stamp this test wrote");
        assert_eq!(store.read_stamp(), None);
    }
}
