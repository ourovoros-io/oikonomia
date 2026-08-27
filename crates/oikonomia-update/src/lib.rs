//! Signed in-app update check for the Oikonomia desktop shell.
//!
//! HTTP lives here, not in `oikonomia-core` and not in `license.rs`.
//! The webview cannot pass a feed URL, endpoint, or public key.

#![forbid(unsafe_code)]

mod client;
mod error;
mod hosts;
mod machine;
mod notes;
mod status;
mod verify;

pub use client::{
    current_updater_platform, default_updater_cache_dir, delete_artifact, download_and_verify,
    perform_check, ArtifactInstaller, CheckOutcome, ClientConfig, VerifiedOffer, UPDATE_FEED_URL,
};
pub use error::{Result, UpdateError};
pub use hosts::HostPolicy;
pub use machine::UpdateMachine;
pub use notes::sanitize_notes;
pub use status::UpdateStatus;
pub use verify::parse_public_key;

#[cfg(test)]
mod tests;
