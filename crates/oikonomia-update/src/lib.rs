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
mod status;
#[cfg(test)]
mod test_macros;
mod verify;

pub use client::{
    ArtifactInstaller, CheckOutcome, ClientConfig, InstallHandoff, InstallOutcome, InstallRoute,
    UPDATE_FEED_URL, VerifiedOffer, current_updater_platform, default_updater_cache_dir,
    delete_artifact, download_and_verify, perform_check,
};
pub use error::{Result, UpdateError};
pub use feed::{FeedArtifact, assemble_manifest};
pub use hosts::HostPolicy;
pub use machine::UpdateMachine;
pub use notes::sanitize_notes;
pub use status::UpdateStatus;
pub use verify::{parse_public_key, verify_manifest_bytes};

#[cfg(test)]
mod tests;
