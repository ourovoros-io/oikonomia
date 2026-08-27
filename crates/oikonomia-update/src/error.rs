//! Typed errors for the update wrapper.

use thiserror::Error;

/// Fallible operations in `oikonomia-update`.
pub type Result<T> = std::result::Result<T, UpdateError>;

/// Failures from check or install. Check maps most of these to [`crate::UpdateStatus::Failed`].
///
/// [`UpdateError::InstallNotAvailable`] is a hard IPC error: `update_install` is
/// illegal unless the machine is [`crate::UpdateStatus::Available`].
#[derive(Debug, Error)]
pub enum UpdateError {
    /// `update_install` was called from Idle, Checking, UpToDate, or Failed.
    #[error("install is only allowed after a check that found an update")]
    InstallNotAvailable,

    /// Baked or supplied updater public key is missing or not a minisign key.
    #[error("updater public key is missing")]
    MissingPublicKey,

    /// Feed or artifact host could not be reached (offline, DNS, timeout, HTTP).
    #[error("update feed could not be reached")]
    Network,

    /// Detached manifest signature was missing or did not verify.
    #[error("update manifest signature is invalid")]
    ManifestSignature,

    /// Manifest JSON was truncated or not an object after a valid signature.
    #[error("update manifest is not valid json")]
    ManifestParse,

    /// Artifact URL is not https or not on the GitHub allow-list (or is a `.deb`).
    #[error("update artifact url is not allow-listed")]
    ArtifactUrl,

    /// Artifact hash or minisign signature did not match.
    #[error("update artifact failed verification")]
    ArtifactIntegrity,

    /// Build-time feed URL failed to parse (programming error).
    #[error("update feed url is invalid")]
    InvalidFeedUrl,
}

impl UpdateError {
    /// Stable machine code for desktop [`CommandError`] mapping.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InstallNotAvailable => "update_install_not_allowed",
            Self::MissingPublicKey => "update_missing_public_key",
            Self::Network => "update_network",
            Self::ManifestSignature => "update_manifest_signature",
            Self::ManifestParse => "update_manifest_parse",
            Self::ArtifactUrl => "update_artifact_url",
            Self::ArtifactIntegrity => "update_artifact_integrity",
            Self::InvalidFeedUrl => "update_invalid_feed_url",
        }
    }
}
