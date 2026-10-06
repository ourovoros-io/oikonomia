//! Signed in-app update check for the Oikonomia desktop shell.
//!
//! HTTP lives here, not in `oikonomia-core`.
//! The webview cannot pass a feed URL, endpoint, or public key.

#![forbid(unsafe_code)]

mod client;
mod error;
pub mod feed;
mod hosts;
mod machine;
mod notes;
pub mod release_set;
mod status;
mod verify;

pub use client::{
    ArtifactInstaller, CheckOutcome, ClientConfig, InstallHandoff, InstallOutcome, InstallRoute,
    UPDATE_FEED_URL, VerifiedOffer, current_updater_platform, delete_artifact, download_and_verify,
    install_offer, perform_check,
};
pub use error::{Result, UpdateError};
pub use feed::{FeedArtifact, assemble_manifest};
pub use hosts::HostPolicy;
pub use machine::{CheckStart, UpdateMachine};
pub use notes::sanitize_notes;
pub use release_set::{
    CHECKSUMS_FILE, ReleaseSetError, WindowsBuild, checksum_line, checksummed_assets, feed_entries,
    feed_platform_keys, fixed_name_copies, fixed_names, is_published_asset,
};
pub use status::UpdateStatus;
pub use verify::{parse_public_key, verify_manifest_bytes, verify_minisign};

#[cfg(test)]
mod tests;
