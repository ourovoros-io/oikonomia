//! The update status the webview is shown.
//!
//! [`UpdateStatus`] is the only update type that crosses IPC. It serializes
//! with a `kind` tag in snake case, and `web/src/lib/updateCheck.ts` decodes
//! exactly that shape, so a variant or field renamed here has to be renamed
//! there. It is a projection of the machine's private state, which holds the
//! artifact URL, signature and digest; none of those is in here.

use serde::{Deserialize, Serialize};

/// Where the session's update check or install stands, as the webview
/// sees it.
///
/// `kind` is the discriminant the webview matches on. The variants that
/// report a newer version carry its version and escaped notes only, never a
/// URL, a signature or a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateStatus {
    /// No check has been requested this session.
    #[default]
    Idle,
    /// A check is in flight.
    Checking,
    /// Nothing newer is published: the verified manifest names this version
    /// or an older one, or the feed server answered 204.
    UpToDate,
    /// The verified manifest offers a newer version this copy may install.
    Available {
        /// The published version from the signed manifest, without a
        /// leading `v`.
        version: String,
        /// The release notes with every HTML markup character escaped.
        notes: String,
    },
    /// Manifest verified and a newer version exists, but the system package
    /// manager owns this install. The app reports it and never installs it.
    AvailableManually {
        /// The published version from the signed manifest, without a
        /// leading `v`.
        version: String,
        /// The release notes with every HTML markup character escaped.
        notes: String,
    },
    /// An install is downloading, verifying or handing over the artifact.
    /// No check and no second install starts until it ends.
    Installing,
    /// The last check or install failed. The rest of the application is
    /// unaffected, and a new check may be started.
    Failed,
}
