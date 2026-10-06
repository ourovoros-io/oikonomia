//! Signed in-app update check for the Oikonomia desktop shell.
//!
//! HTTP lives here, not in `oikonomia-core`.
//! The webview cannot pass a feed URL, endpoint, or public key.

#![forbid(unsafe_code)]

mod client;
mod error;
mod feed;
mod hosts;
mod machine;
mod notes;
mod release_set;
mod status;
mod verify;
mod version;

pub use crate::client::{
    ArtifactInstaller, CheckOutcome, ClientConfig, InstallHandoff, InstallOutcome, InstallRoute,
    VerifiedOffer, install_offer, perform_check,
};
pub use crate::error::{Result, UpdateError};
pub use crate::feed::{FeedArtifact, assemble_manifest};
pub use crate::machine::{CheckStart, UpdateMachine};
pub use crate::release_set::{
    ReleaseSetError, WindowsBuild, checksum_line, checksummed_assets, feed_entries,
    feed_platform_keys, fixed_name_copies, fixed_names, is_published_asset,
};
pub use crate::status::UpdateStatus;
pub use crate::verify::{sha256_hex, verify_signature};

#[cfg(test)]
mod tests;
