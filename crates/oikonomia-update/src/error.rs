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
    /// `update_install` was called from Idle, Checking, `UpToDate`, or Failed.
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

    /// A newer version is published, but the manifest lists no artifact for
    /// the platform this copy runs on.
    #[error("update feed has no artifact for this platform")]
    MissingPlatform,

    /// Artifact URL is not https or not on the GitHub allow-list (or is a `.deb`).
    #[error("update artifact url is not allow-listed")]
    ArtifactUrl,

    /// Artifact hash or minisign signature did not match.
    #[error("update artifact failed verification")]
    ArtifactIntegrity,

    /// The artifact is larger than this copy is willing to download. Only
    /// the artifact gets this code: an oversized manifest or signature is a
    /// broken feed and stays [`UpdateError::Network`].
    #[error("update artifact is larger than this copy can download")]
    ArtifactTooLarge,

    /// Build-time feed URL failed to parse (programming error).
    #[error("update feed url is invalid")]
    InvalidFeedUrl,
}

impl UpdateError {
    /// Every code [`UpdateError::code`] can return.
    ///
    /// The desktop crate checks this list against the codes the UI has copy for.
    pub const ALL_CODES: &'static [&'static str] = &[
        "update_install_not_allowed",
        "update_missing_public_key",
        "update_network",
        "update_manifest_signature",
        "update_manifest_parse",
        "update_missing_platform",
        "update_artifact_url",
        "update_artifact_integrity",
        "update_artifact_too_large",
        "update_invalid_feed_url",
    ];

    /// Returns the stable machine code the desktop crate sends to the UI for this error.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InstallNotAvailable => "update_install_not_allowed",
            Self::MissingPublicKey => "update_missing_public_key",
            Self::Network => "update_network",
            Self::ManifestSignature => "update_manifest_signature",
            Self::ManifestParse => "update_manifest_parse",
            Self::MissingPlatform => "update_missing_platform",
            Self::ArtifactUrl => "update_artifact_url",
            Self::ArtifactIntegrity => "update_artifact_integrity",
            Self::ArtifactTooLarge => "update_artifact_too_large",
            Self::InvalidFeedUrl => "update_invalid_feed_url",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UpdateError;
    use crate::test_macros::listed_variants;

    listed_variants! {
        units listed_errors for UpdateError {
            UpdateError::InstallNotAvailable,
            UpdateError::MissingPublicKey,
            UpdateError::Network,
            UpdateError::ManifestSignature,
            UpdateError::ManifestParse,
            UpdateError::MissingPlatform,
            UpdateError::ArtifactUrl,
            UpdateError::ArtifactIntegrity,
            UpdateError::ArtifactTooLarge,
            UpdateError::InvalidFeedUrl,
        }
    }

    /// Fails unless `ALL_CODES` is the codes of the variants in the
    /// `listed_errors` list above, in that order, each once. The compiler
    /// checks that list against the enum with an exhaustive `match`, so a
    /// variant added to the enum but left out of the list does not compile. It
    /// does not check the wording of a code or that the UI has copy for it; the
    /// desktop crate does.
    #[test]
    fn all_codes_lists_exactly_the_code_of_every_variant() {
        let listed = listed_errors::variants();
        let codes: Vec<&str> = listed.iter().map(UpdateError::code).collect();

        listed_errors::assert_every_position_once(
            listed.iter().map(listed_errors::position).collect(),
        );
        assert_eq!(codes, UpdateError::ALL_CODES);
    }
}
