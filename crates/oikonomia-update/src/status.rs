//! Update state machine status, serde-tagged for the webview.

use serde::{Deserialize, Serialize};

/// Unlock-screen update machine. Never a pile of bools.
///
/// `kind` is the discriminant the webview matches on. [`UpdateStatus::Available`]
/// carries version and sanitized notes only — never a URL or pubkey.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateStatus {
    /// No check has been requested this session.
    #[default]
    Idle,
    /// `update_check` is in flight.
    Checking,
    /// Manifest verified; installed version is current (or the feed returned 204).
    UpToDate,
    /// Manifest verified and a newer artifact URL passed the allow-list.
    Available {
        /// Remote `SemVer` from the signed manifest.
        version: String,
        /// Release notes, HTML-stripped and escaped. Plain text only.
        notes: String,
    },
    /// Manifest verified and a newer version exists, but the system package
    /// manager owns this install. The app reports it and never installs it.
    AvailableManually {
        /// Remote `SemVer` from the signed manifest.
        version: String,
        /// Release notes, HTML-stripped and escaped. Plain text only.
        notes: String,
    },
    /// `update_install` is downloading, verifying or handing over the
    /// artifact. No check and no second install starts until it ends.
    Installing,
    /// Check or install failed. Unlock and export stay usable.
    Failed,
}
