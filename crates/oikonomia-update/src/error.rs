//! The error type of the crate and the stable codes the UI words.
//!
//! One public enum, because the desktop crate turns each variant into a code
//! ([`UpdateError::code`]) that the web UI has copy for. The codes are a
//! contract with `web/src/lib/errorCodes.json`: [`UpdateError::ALL_CODES`]
//! lists them, and a desktop test compares that list with the fixture.
//!
//! A failed check or install hands its error to the caller inside the
//! outcome: [`perform_check`] in [`CheckOutcome::Failed`] and
//! [`install_offer`] in [`InstallOutcome::Failed`]. This crate does not log
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
    ///
    /// Also returned when the feed or its detached signature is larger than
    /// its size limit (`MAX_MANIFEST_BYTES`, `MAX_SIGNATURE_BYTES`): an
    /// oversized manifest or signature is a broken feed, and a broken feed
    /// is reported like one that could not be fetched. An oversized artifact
    /// is [`Self::ArtifactTooLarge`].
    #[error("update server could not be reached or gave an unusable answer")]
    Network,

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

    /// The artifact is larger than this copy is willing to download
    /// (`MAX_ARTIFACT_BYTES`). Retrying cannot help, so it is kept apart from
    /// [`Self::Network`]. Only the artifact gets this code: an oversized
    /// manifest or signature is a broken feed and stays [`Self::Network`].
    #[error("update artifact is larger than this copy can download")]
    ArtifactTooLarge,

    /// The cache directory could not be created or made private, or the
    /// verified artifact could not be written into it.
    #[error("update cache could not be written")]
    CacheIo(#[source] std::io::Error),

    /// The artifact was downloaded and verified, and putting it in place of
    /// the running copy failed at `step`.
    ///
    /// Returned by the [`ArtifactInstaller`](crate::ArtifactInstaller) the
    /// desktop crate supplies; nothing in this crate constructs it. The
    /// operating-system error behind a step is logged by that installer and
    /// is not carried here.
    #[error("update could not be installed: cannot {step}")]
    InstallFailed {
        /// The step that failed.
        step: InstallStep,
    },

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
        "update_manifest_signature",
        "update_manifest_parse",
        "update_invalid_version",
        "update_missing_platform",
        "update_artifact_url",
        "update_artifact_integrity",
        "update_artifact_too_large",
        "update_cache_io",
        "update_install_failed",
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
            Self::ManifestSignature => "update_manifest_signature",
            Self::ManifestParse => "update_manifest_parse",
            Self::InvalidVersion { .. } => "update_invalid_version",
            Self::MissingPlatform => "update_missing_platform",
            Self::ArtifactUrl => "update_artifact_url",
            Self::ArtifactIntegrity => "update_artifact_integrity",
            Self::ArtifactTooLarge => "update_artifact_too_large",
            Self::CacheIo(_) => "update_cache_io",
            Self::InstallFailed { .. } => "update_install_failed",
            Self::InvalidFeedUrl => "update_invalid_feed_url",
            Self::InvalidFeedInput { .. } => "update_invalid_feed_input",
        }
    }
}

/// A step of putting a verified artifact in place of the running copy.
///
/// Which steps an install has depends on how the copy was installed: an
/// archive is unpacked, an image is staged, an installer is started. The
/// desktop crate's installer says which step each of its kinds can fail at.
///
/// Exhaustive on purpose, like [`UpdateError`]: a new step should stop the
/// tests that list the steps from compiling until it is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStep {
    /// Finding the verified artifact where the client wrote it.
    FindArtifact,
    /// Finding what is to be replaced: the running executable, and the
    /// bundle or image that holds it.
    FindRunningCopy,
    /// Unpacking the artifact into the one app it must hold.
    Unpack,
    /// Copying the artifact to a staging file beside the running copy.
    Stage,
    /// Making the staged copy executable.
    SetPermissions,
    /// Moving the new copy into the place of the running one.
    Replace,
    /// Starting the installer program.
    StartInstaller,
}

impl InstallStep {
    /// Every step, for tests that check each has its own wording.
    pub const ALL: &'static [Self] = &[
        Self::FindArtifact,
        Self::FindRunningCopy,
        Self::Unpack,
        Self::Stage,
        Self::SetPermissions,
        Self::Replace,
        Self::StartInstaller,
    ];
}

impl std::fmt::Display for InstallStep {
    /// Writes the step as the words that follow "cannot" in the message of
    /// [`UpdateError::InstallFailed`].
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FindArtifact => "find the downloaded update",
            Self::FindRunningCopy => "find the running copy",
            Self::Unpack => "unpack the update",
            Self::Stage => "copy the update beside the running copy",
            Self::SetPermissions => "make the new copy executable",
            Self::Replace => "move the new copy into place",
            Self::StartInstaller => "start the installer",
        })
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
    use super::{InstallStep, UpdateError};
    use oikonomia_test_support::listed_variants;
    use std::error::Error;

    /// One sample of every variant, in the order of `ALL_CODES`.
    fn every_variant() -> Vec<UpdateError> {
        vec![
            UpdateError::InstallNotAvailable,
            UpdateError::MissingPublicKey,
            UpdateError::Network,
            UpdateError::ManifestSignature,
            UpdateError::ManifestParse,
            UpdateError::InvalidVersion {
                version: "x".to_owned(),
            },
            UpdateError::MissingPlatform,
            UpdateError::ArtifactUrl,
            UpdateError::ArtifactIntegrity,
            UpdateError::ArtifactTooLarge,
            UpdateError::CacheIo(std::io::Error::other("x")),
            UpdateError::InstallFailed {
                step: InstallStep::Replace,
            },
            UpdateError::InvalidFeedUrl,
            UpdateError::InvalidFeedInput { field: "x" },
        ]
    }

    listed_variants! {
        patterns listed_errors for UpdateError {
            UpdateError::InstallNotAvailable,
            UpdateError::MissingPublicKey,
            UpdateError::Network,
            UpdateError::ManifestSignature,
            UpdateError::ManifestParse,
            UpdateError::InvalidVersion { .. },
            UpdateError::MissingPlatform,
            UpdateError::ArtifactUrl,
            UpdateError::ArtifactIntegrity,
            UpdateError::ArtifactTooLarge,
            UpdateError::CacheIo(_),
            UpdateError::InstallFailed { .. },
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

    listed_variants! {
        units listed_steps for InstallStep {
            InstallStep::FindArtifact,
            InstallStep::FindRunningCopy,
            InstallStep::Unpack,
            InstallStep::Stage,
            InstallStep::SetPermissions,
            InstallStep::Replace,
            InstallStep::StartInstaller,
        }
    }

    #[test]
    fn a_failed_install_says_which_step_failed_and_every_step_reads_differently() {
        let messages: Vec<String> = InstallStep::ALL
            .iter()
            .map(|step| UpdateError::InstallFailed { step: *step }.to_string())
            .collect();

        assert_eq!(InstallStep::ALL.len(), listed_steps::COUNT);
        listed_steps::assert_every_position_once(
            InstallStep::ALL
                .iter()
                .map(listed_steps::position)
                .collect(),
        );
        assert_eq!(
            messages[listed_steps::position(&InstallStep::StartInstaller)],
            "update could not be installed: cannot start the installer"
        );

        let mut distinct = messages.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), messages.len(), "{messages:?}");
        for message in &messages {
            assert_eq!(*message, message.to_lowercase());
            assert!(!message.ends_with('.'), "{message}");
        }
    }

    #[test]
    fn a_failed_install_is_not_reported_as_a_failed_verification() {
        let error = UpdateError::InstallFailed {
            step: InstallStep::Stage,
        };

        assert_eq!(error.code(), "update_install_failed");
        assert_ne!(error.code(), UpdateError::ArtifactIntegrity.code());
        assert!(error.source().is_none());
    }

    #[test]
    fn a_cache_failure_keeps_its_cause_out_of_the_message() {
        let error = UpdateError::CacheIo(std::io::Error::other("disk on fire"));

        assert_eq!(error.to_string(), "update cache could not be written");
        let source = error.source().expect("the i/o error is the source");
        assert_eq!(source.to_string(), "disk on fire");
    }
}
