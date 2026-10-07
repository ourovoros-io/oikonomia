//! Signed update check and install for the Oikonomia desktop application.
//!
//! This crate is the only code in the application that opens a network
//! connection. `oikonomia-core` stays offline; the desktop crate calls in
//! here when the user clicks "Check for updates", and never otherwise. The
//! webview takes no part in it: it cannot supply a feed URL, a host or a
//! key, and it is shown only an [`UpdateStatus`].
//!
//! The crate also holds the release-side half of the same contract
//! ([`assemble_manifest`], the release-set functions, the artifact size
//! check and the `assemble_feed` binary). The client's tests parse what the
//! release side writes, and the release side checks a feed, its signatures,
//! digests and sizes before publication with the functions and the limit
//! the client uses ([`verify_signature`], [`sha256_hex`],
//! [`check_feed_as_client`] and [`check_artifact_file`]), so the two cannot
//! drift apart.
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
//! The check downloads no artifact and writes no file. A step that fails
//! ends it as [`CheckOutcome::Failed`], which holds the [`UpdateError`].
//!
//! # Install
//!
//! [`install_offer`] takes the offer of a check and does these steps in
//! order, stopping at the first that fails:
//!
//! 1. Creates the cache directory and, on Unix, sets its mode to `0700`.
//! 2. Removes the files earlier installs left in it, except one under the
//!    name this artifact will have: the digest from the signed feed, then
//!    the artifact's file name.
//! 3. Looks for a file under that name, which an earlier attempt at the same
//!    release may have left. It is read only if it is a regular file, not a
//!    link, and on Unix one that belongs to the owner of the cache directory
//!    and grants nothing to group or others. When its
//!    bytes pass the checks of steps 5 and 6, the install continues at
//!    step 8 with that file and downloads nothing. A regular file there that
//!    is longer than [`MAX_ARTIFACT_BYTES`] cannot be the artifact of an
//!    install this copy would make: the install fails as
//!    [`UpdateError::ArtifactTooLarge`] and the file is removed.
//! 4. Checks the artifact URL against the allow-list again and downloads the
//!    artifact into memory.
//! 5. Compares the SHA-256 of the bytes with the one in the signed feed.
//! 6. Verifies the artifact's minisign signature over the same bytes.
//! 7. Writes the bytes to a new file in the cache directory, created with
//!    `create_new` and, on Unix, mode `0600`, and renames it to the
//!    artifact's name. The rename replaces a file or a link that step 3
//!    found and did not use: a planted symbolic link is removed, never
//!    written through. A directory at that name is not replaced, and the
//!    install fails here.
//! 8. Hands the path to the caller's [`ArtifactInstaller`].
//! 9. Deletes the file, unless the installer reports a separate installer
//!    process that is still running from it.
//!
//! After a failure at any step the artifact, if it was written at all, is
//! removed, and the install ends as [`InstallOutcome::Failed`], which holds
//! the [`UpdateError`].
//!
//! Neither function logs its outcome. The caller has the error and decides
//! what to record of it. The one thing logged here is the transport error of
//! a request that got no response: [`UpdateError::Network`] does not carry
//! it, so it would be lost where it is dropped.
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
//! - [`MAX_ARTIFACT_BYTES`], 200 MiB, for the artifact, which is held in
//!   memory until it is verified. The release lane reads the same constant
//!   through [`check_artifact_file`], so a release cannot publish an
//!   installer the client would refuse to download.
//!
//! An artifact over its limit fails as [`UpdateError::ArtifactTooLarge`]. A
//! feed or signature over its limit is a broken feed and fails as
//! [`UpdateError::Network`].
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
//! the one a still-running installer was started from. Nothing in it is
//! trusted for being there. The one file ever read back is the one under the
//! name of the artifact being installed, and its bytes are used only after
//! the digest and signature checks a download gets; a retry of the same
//! release therefore neither fails on that file nor downloads it again.
//!
//! # State
//!
//! [`UpdateMachine`] tracks one session: idle, checking, up to date,
//! available (to install, or only to report), installing or failed. The
//! desktop keeps it behind a mutex and
//! never holds that mutex across a request: it begins a step, releases the
//! machine, does the work, and finishes the step. Of a failure the machine
//! keeps the error's code, and [`UpdateStatus::Failed`] carries it to the
//! webview, which words it; a step whose task died is abandoned and has
//! none.
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

mod artifact_limit;
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

pub use crate::artifact_limit::{ArtifactSizeError, MAX_ARTIFACT_BYTES, check_artifact_file};
pub use crate::client::{
    ArtifactInstaller, CheckOutcome, ClientConfig, InstallHandoff, InstallOutcome, InstallRoute,
    VerifiedOffer, check_feed_as_client, install_offer, perform_check,
};
pub use crate::error::{FeedRefusal, InstallStep, Result, UpdateError};
pub use crate::feed::{FeedArtifact, assemble_manifest};
pub use crate::machine::{CheckStart, UpdateMachine};
pub use crate::release_set::{
    ReleaseSetError, UpdaterArtifactKind, WindowsBuild, checksum_line, checksummed_assets,
    feed_entries, feed_platform_keys, fixed_name_copies, fixed_names, is_published_asset,
    updater_artifact_kinds,
};
pub use crate::status::UpdateStatus;
pub use crate::verify::{sha256_hex, verify_signature};

#[cfg(test)]
mod tests;
