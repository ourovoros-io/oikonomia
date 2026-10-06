//! The error type of the crate and the stable codes the UI words.
//!
//! One public enum, because the desktop crate turns each variant into a code
//! ([`UpdateError::code`]) that the web UI has copy for. The codes are a
//! contract with `web/src/lib/errorCodes.json`: [`UpdateError::ALL_CODES`]
//! lists them, and a desktop test compares that list with the fixture.
//!
//! A failed check or install hands its error to the caller inside the
//! outcome: [`perform_check`] in [`CheckOutcome::Failed`] and
//! [`install_offer`] in [`InstallOutcome::Failed`]. Nothing in this crate logs
//! it. The desktop crate logs it and gives it to the [`UpdateMachine`], which
//! keeps the code for the [`UpdateStatus`] the webview is shown. The other
//! errors are returned as errors: from building a [`ClientConfig`], from
//! [`UpdateMachine::begin_install`] and from the release-side functions.
//!
//! [`FeedRefusal`] is the release lane's: it says which entry of a feed
//! installed copies would refuse, and wraps the [`UpdateError`] they would
//! refuse it with. It never crosses IPC and has no code.
//!
//! [`perform_check`]: crate::perform_check
//! [`install_offer`]: crate::install_offer
//! [`CheckOutcome::Failed`]: crate::CheckOutcome::Failed
//! [`InstallOutcome::Failed`]: crate::InstallOutcome::Failed
//! [`ClientConfig`]: crate::ClientConfig
//! [`UpdateMachine`]: crate::UpdateMachine
//! [`UpdateStatus`]: crate::UpdateStatus
//! [`UpdateMachine::begin_install`]: crate::UpdateMachine::begin_install

use thiserror::Error;

/// The result of a fallible operation in this crate.
pub type Result<T> = std::result::Result<T, UpdateError>;

/// A failure of the update check, the install, or the release-side feed
/// assembly.
///
/// The enum is exhaustive on purpose. The crate is not published, and the
/// desktop crate maps every variant to a code, so a new variant should stop
/// that crate from compiling until it is handled.
#[derive(Debug, Error)]
pub enum UpdateError {
    /// An install was begun while the machine held no offer this copy may
    /// install: from Idle, Checking, `UpToDate`, `AvailableManually`,
    /// Installing, or Failed.
    #[error("install is only allowed after a check that found an update")]
    InstallNotAvailable,

    /// The updater public key is empty, or is neither a minisign public key
    /// nor the base64 of one.
    #[error("updater public key is missing or invalid")]
    MissingPublicKey,

    /// A request did not end in a usable response: the host could not be
    /// reached, the request timed out, the status was neither 200 nor 204
    /// (but see [`Self::ManifestSignature`] for a signature that is not
    /// there), an artifact request was answered 204, a redirect had no usable
    /// `Location`, there were more redirects than the limit, or the body could
    /// not be read to its end.
    #[error("update server could not be reached or answered with an error")]
    Network,

    /// A response body was larger than the limit for what was requested.
    #[error("update response is larger than its size limit")]
    ResponseTooLarge,

    /// The detached signature of the manifest is absent (the server answered
    /// 404 or 204 for it), is not a minisign signature, or does not verify
    /// the manifest with the updater public key. Also returned when a
    /// manifest names an artifact with an empty signature.
    #[error("update manifest signature is invalid")]
    ManifestSignature,

    /// The manifest carries a valid signature but is not the JSON object the
    /// client reads: it is truncated, or a required field is missing or has
    /// the wrong type.
    #[error("update manifest is not valid json")]
    ManifestParse,

    /// A version is not `SemVer`: the running version given to the client,
    /// the version in a signed manifest, or the version given to
    /// [`assemble_manifest`](crate::assemble_manifest).
    #[error("version {version:?} is not semver")]
    InvalidVersion {
        /// The text that did not parse, after trimming.
        version: String,
    },

    /// A newer version is published, but the manifest lists no artifact for
    /// the platform this copy runs on.
    #[error("update feed has no artifact for this platform")]
    MissingPlatform,

    /// A URL is not on the allow-list: it is not https, its host is not a
    /// GitHub release host, or it is an artifact URL ending in `.deb`. Applies
    /// to the feed URL, to the artifact URL, and to every redirect from either.
    #[error("update url is not allow-listed")]
    ArtifactUrl,

    /// The artifact does not have the SHA-256 the manifest states, its
    /// signature does not verify, or a SHA-256 is not 64 hex characters.
    #[error("update artifact failed verification")]
    ArtifactIntegrity,

    /// The cache directory could not be created or made private, or the
    /// verified artifact could not be written into it.
    #[error("update cache could not be written")]
    CacheIo(#[source] std::io::Error),

    /// The feed URL built into the crate does not parse. No input reaches
    /// this; it guards the constant.
    #[error("update feed url is invalid")]
    InvalidFeedUrl,

    /// An input to [`assemble_manifest`](crate::assemble_manifest) is empty.
    #[error("feed input is empty: {field}")]
    InvalidFeedInput {
        /// Which input was empty.
        field: &'static str,
    },
}

impl UpdateError {
    /// Every code [`UpdateError::code`] can return.
    ///
    /// The desktop crate checks this list against the codes the UI has copy for.
    pub const ALL_CODES: &'static [&'static str] = &[
        "update_install_not_allowed",
        "update_missing_public_key",
        "update_network",
        "update_response_too_large",
        "update_manifest_signature",
        "update_manifest_parse",
        "update_invalid_version",
        "update_missing_platform",
        "update_artifact_url",
        "update_artifact_integrity",
        "update_cache_io",
        "update_invalid_feed_url",
        "update_invalid_feed_input",
    ];

    /// Returns the stable machine code the desktop crate sends to the UI for this error.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InstallNotAvailable => "update_install_not_allowed",
            Self::MissingPublicKey => "update_missing_public_key",
            Self::Network => "update_network",
            Self::ResponseTooLarge => "update_response_too_large",
            Self::ManifestSignature => "update_manifest_signature",
            Self::ManifestParse => "update_manifest_parse",
            Self::InvalidVersion { .. } => "update_invalid_version",
            Self::MissingPlatform => "update_missing_platform",
            Self::ArtifactUrl => "update_artifact_url",
            Self::ArtifactIntegrity => "update_artifact_integrity",
            Self::CacheIo(_) => "update_cache_io",
            Self::InvalidFeedUrl => "update_invalid_feed_url",
            Self::InvalidFeedInput { .. } => "update_invalid_feed_input",
        }
    }
}

/// Why installed copies would refuse a feed, as
/// [`check_feed_as_client`](crate::check_feed_as_client) reports it to the
/// release lane.
///
/// It names the platform and the URL, which [`UpdateError`] does not: the
/// person promoting a release needs to know which entry to fix. The cause
/// is the error a copy would end its check with.
///
/// Exhaustive on purpose, like [`UpdateError`].
#[derive(Debug, Error)]
pub enum FeedRefusal {
    /// No copy can read the feed: it is not the JSON the client expects, or
    /// its version is not `SemVer`.
    #[error("installed copies cannot read the feed")]
    Unreadable(#[source] UpdateError),

    /// The feed has no entry for a platform that was to be checked.
    #[error("{platform}: the feed has no entry for this platform")]
    MissingPlatform {
        /// The platform key that is missing.
        platform: String,
    },

    /// Copies on one platform would refuse their entry.
    #[error("{platform}: installed copies refuse {url}")]
    Entry {
        /// The platform key of the entry.
        platform: String,
        /// The artifact URL the entry gives, as written in the feed.
        url: String,
        /// What a copy would end its check with.
        #[source]
        source: UpdateError,
    },
}

#[cfg(test)]
mod tests {
    use super::UpdateError;
    use oikonomia_test_support::listed_variants;
    use std::error::Error;

    /// One sample of every variant, in the order of `ALL_CODES`.
    fn every_variant() -> Vec<UpdateError> {
        vec![
            UpdateError::InstallNotAvailable,
            UpdateError::MissingPublicKey,
            UpdateError::Network,
            UpdateError::ResponseTooLarge,
            UpdateError::ManifestSignature,
            UpdateError::ManifestParse,
            UpdateError::InvalidVersion {
                version: "x".to_owned(),
            },
            UpdateError::MissingPlatform,
            UpdateError::ArtifactUrl,
            UpdateError::ArtifactIntegrity,
            UpdateError::CacheIo(std::io::Error::other("x")),
            UpdateError::InvalidFeedUrl,
            UpdateError::InvalidFeedInput { field: "x" },
        ]
    }

    listed_variants! {
        patterns listed_errors for UpdateError {
            UpdateError::InstallNotAvailable,
            UpdateError::MissingPublicKey,
            UpdateError::Network,
            UpdateError::ResponseTooLarge,
            UpdateError::ManifestSignature,
            UpdateError::ManifestParse,
            UpdateError::InvalidVersion { .. },
            UpdateError::MissingPlatform,
            UpdateError::ArtifactUrl,
            UpdateError::ArtifactIntegrity,
            UpdateError::CacheIo(_),
            UpdateError::InvalidFeedUrl,
            UpdateError::InvalidFeedInput { .. },
        }
    }

    /// Fails when `every_variant` has no sample for a variant named in the
    /// `listed_errors` list above, or when `ALL_CODES` is not the codes of
    /// the samples, in order. The compiler checks `listed_errors` against the
    /// enum with an exhaustive `match`, so a variant added to the enum but
    /// left out of the list does not compile. It does not check the wording
    /// of a code or that the UI has copy for it; the desktop crate does.
    #[test]
    fn all_codes_lists_exactly_the_code_of_every_variant() {
        let samples = every_variant();
        let codes: Vec<&str> = samples.iter().map(UpdateError::code).collect();

        listed_errors::assert_every_position_once(
            samples.iter().map(listed_errors::position).collect(),
        );
        assert_eq!(codes, UpdateError::ALL_CODES);
    }

    #[test]
    fn every_code_is_distinct_and_prefixed() {
        let mut codes = UpdateError::ALL_CODES.to_vec();
        codes.sort_unstable();
        codes.dedup();

        assert_eq!(codes.len(), UpdateError::ALL_CODES.len());
        for code in codes {
            assert!(code.starts_with("update_"), "{code}");
        }
    }

    #[test]
    fn a_cache_failure_keeps_its_cause_out_of_the_message() {
        let error = UpdateError::CacheIo(std::io::Error::other("disk on fire"));

        assert_eq!(error.to_string(), "update cache could not be written");
        let source = error.source().expect("the i/o error is the source");
        assert_eq!(source.to_string(), "disk on fire");
    }
}
