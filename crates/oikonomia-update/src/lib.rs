//! Signed update check and install for the Oikonomia desktop application.
//!
//! This crate is the only code in the application that opens a network
//! connection. `oikonomia-core` stays offline; the desktop crate calls in
//! here when the user clicks "Check for updates", and never otherwise. The
//! webview takes no part in it: it cannot supply a feed URL, a host or a
//! key, and it is shown only an [`UpdateStatus`].
//!
//! The crate also holds the release-side half of the same contract
//! ([`assemble_manifest`], the release-set functions and the `assemble_feed`
//! binary), so that the feed is written, and checked before publication, by
//! the code that reads it.
//!
//! # Trust root
//!
//! One minisign key pair. The secret key signs each artifact and the feed in
//! the release workflow. The public key is compiled into the desktop crate
//! (`UPDATER_PUBLIC_KEY` in its `update_key.rs`) and handed to
//! [`ClientConfig::production`]; this crate contains no key and reads none
//! from disk or from the network. Nothing else is trusted: not GitHub, which
//! only stores the files, and not TLS, which only carries them. A file is
//! acted on only if that key signed it.
//!
//! # Check
//!
//! [`perform_check`] does these steps in order and stops at the first that
//! fails:
//!
//! 1. Fetches the feed, `latest.json`, from the URL built into the crate
//!    (`UPDATE_FEED_URL`). The request names the running version and
//!    platform in its query. A 204 answer ends the check as up to date.
//! 2. Fetches the detached signature from the same URL with `.sig` appended.
//!    A 404 or 204 here fails the check: a feed without a signature is not
//!    trusted.
//! 3. Verifies the signature over the feed's bytes exactly as they arrived.
//!    Nothing parses the feed before this passes.
//! 4. Parses the feed as JSON.
//! 5. Parses its version and compares it with the running one. A version
//!    that is not greater ends the check as up to date, so an older release
//!    is never offered.
//! 6. Looks up the entry for this platform (`{os}-{arch}`, such as
//!    `darwin-aarch64`). A newer version without one fails the check.
//! 7. Checks the entry: its artifact URL must be on the allow-list and must
//!    not be a `.deb`, its signature must be present, and its SHA-256 must
//!    be 64 hex characters. The release notes are escaped.
//! 8. Returns [`CheckOutcome::Available`] with a [`VerifiedOffer`], or, for a
//!    copy the system package manager owns,
//!    [`CheckOutcome::AvailableManually`] with the version and notes only.
//!
//! The check downloads no artifact and writes no file.
//!
//! # Install
//!
//! [`install_offer`] takes the offer of a check and does these steps in
//! order, stopping at the first that fails:
//!
//! 1. Creates the cache directory and, on Unix, sets its mode to `0700`.
//! 2. Removes the files earlier installs left in it.
//! 3. Checks the artifact URL against the allow-list again and downloads the
//!    artifact into memory.
//! 4. Compares the SHA-256 of the bytes with the one in the signed feed.
//! 5. Verifies the artifact's minisign signature over the same bytes.
//! 6. Writes the bytes to a new file in the cache directory, named after the
//!    digest and the artifact. The file is created with `create_new`, so an
//!    existing path or a planted symbolic link is refused, and on Unix with
//!    mode `0600`.
//! 7. Hands the path to the caller's [`ArtifactInstaller`].
//! 8. Deletes the file, unless the installer reports a separate installer
//!    process that is still running from it.
//!
//! After a failure at any step the artifact, if it was written at all, is
//! removed.
//!
//! # Host allow-list
//!
//! Every URL is checked before it is requested: the feed, its signature,
//! the artifact, and each hop of every redirect, which the client follows
//! itself for that purpose (at most `MAX_REDIRECTS`, five). Production
//! allows https to the hosts GitHub serves release assets from, and nothing
//! else. The list does not decide what is trusted; the signature does. It
//! decides where the app will connect at all, so that a redirect or a wrong
//! URL in a signed feed cannot send a request, with the version and platform
//! in it, to an unrelated host.
//!
//! # Limits
//!
//! Sizes are counted as a body arrives, not taken from `Content-Length`:
//!
//! - `MAX_MANIFEST_BYTES`, 1 MiB, for the feed;
//! - `MAX_SIGNATURE_BYTES`, 16 KiB, for its signature;
//! - `MAX_ARTIFACT_BYTES`, 200 MiB, for the artifact, which is held in
//!   memory until it is verified.
//!
//! Times:
//!
//! - `METADATA_DEADLINE`, 20 seconds, for one whole feed or signature
//!   request, from connecting to the last byte;
//! - `ARTIFACT_CONNECT_TIMEOUT`, 20 seconds, to open the connection for an
//!   artifact;
//! - `ARTIFACT_READ_TIMEOUT`, 30 seconds, for each read of an artifact body.
//!   A download that keeps delivering has no overall limit; one that stalls
//!   fails.
//!
//! A redirect starts a new request with the same limits. A DNS lookup is not
//! bounded by any of them, because the HTTP library cannot interrupt one.
//!
//! # Cache directory
//!
//! The caller chooses the directory and must choose one that only the
//! current user can write to: under the user's own cache location, never a
//! shared temporary directory and never the vault's data directory. The
//! verified artifact is run from there by path, so whoever can write to the
//! directory can replace it between the write and the run. On Unix this
//! crate enforces the mode, and so fails the install when the directory
//! belongs to another account; on Windows it relies on the caller's choice.
//!
//! Each install starts by removing the files earlier ones left, so the
//! directory normally holds only the artifact of the install in flight, or
//! the one a still-running installer was started from. Nothing in it is ever
//! read back as trusted input: each install downloads and verifies again.
//!
//! # State
//!
//! [`UpdateMachine`] tracks one session: idle, checking, up to date,
//! available (to install, or only to report), installing or failed. The
//! desktop keeps it behind a mutex and
//! never holds that mutex across a request: it begins a step, releases the
//! machine, does the work, and finishes the step.
//!
//! # Not defended
//!
//! Both of these withhold an update. Neither can make the client install
//! anything, because an install still needs a signature from the key.
//!
//! - **Freezing a client on its version.** A feed carries no expiry and the
//!   client keeps no record of the newest feed it has seen. Whoever can
//!   answer the feed request can replay an older feed with its valid
//!   signature; the client finds no greater version and reports that it is
//!   up to date.
//! - **An unsigned 204.** The answer "nothing newer", given as HTTP 204 to
//!   the feed request, carries no signature, so whoever can answer that
//!   request can give it.
//!
//! # Stability
//!
//! The crate is not published and has two consumers, the desktop crate and
//! its own `assemble_feed` binary. Its public enums are exhaustive on
//! purpose: adding a state, an outcome or an error should stop the desktop
//! crate from compiling until it handles the addition.

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
