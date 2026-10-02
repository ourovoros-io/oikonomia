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
    /// Every code [`UpdateError::code`] can return.
    ///
    /// The desktop crate checks this list against the codes the UI has copy for.
    pub const ALL_CODES: &'static [&'static str] = &[
        "update_install_not_allowed",
        "update_missing_public_key",
        "update_network",
        "update_manifest_signature",
        "update_manifest_parse",
        "update_artifact_url",
        "update_artifact_integrity",
        "update_invalid_feed_url",
    ];

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

#[cfg(test)]
mod tests {
    use super::UpdateError;

    /// How many variants [`UpdateError`] has. Kept by hand, next to the index
    /// below: it is the number the indices must reach.
    const VARIANT_COUNT: usize = 8;

    /// The position of a variant, from an exhaustive `match` with no wildcard
    /// arm. Adding a variant stops compiling here until it is given the next
    /// index.
    fn variant_index(error: &UpdateError) -> usize {
        match error {
            UpdateError::InstallNotAvailable => 0,
            UpdateError::MissingPublicKey => 1,
            UpdateError::Network => 2,
            UpdateError::ManifestSignature => 3,
            UpdateError::ManifestParse => 4,
            UpdateError::ArtifactUrl => 5,
            UpdateError::ArtifactIntegrity => 6,
            UpdateError::InvalidFeedUrl => 7,
        }
    }

    /// One value of every variant, in declaration order.
    fn every_variant() -> Vec<UpdateError> {
        vec![
            UpdateError::InstallNotAvailable,
            UpdateError::MissingPublicKey,
            UpdateError::Network,
            UpdateError::ManifestSignature,
            UpdateError::ManifestParse,
            UpdateError::ArtifactUrl,
            UpdateError::ArtifactIntegrity,
            UpdateError::InvalidFeedUrl,
        ]
    }

    /// Fails when a variant has no sample, a code is missing from `ALL_CODES`,
    /// or the two lists differ in order. The samples must carry exactly the
    /// indices `0..VARIANT_COUNT`, and `ALL_CODES` must be their codes in that
    /// order. It does not check the wording of a code or that the UI has copy
    /// for it (the desktop crate does), nor that `VARIANT_COUNT` was raised for
    /// a new variant.
    #[test]
    fn all_codes_lists_exactly_the_code_of_every_variant() {
        let samples = every_variant();
        let indices: Vec<usize> = samples.iter().map(variant_index).collect();
        let codes: Vec<&str> = samples.iter().map(UpdateError::code).collect();

        assert_eq!(indices, (0..VARIANT_COUNT).collect::<Vec<_>>());
        assert_eq!(codes, UpdateError::ALL_CODES);
    }
}
