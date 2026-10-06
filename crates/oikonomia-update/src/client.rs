//! The update client: checks the signed feed and installs what it offers.
//!
//! Everything that touches the network is in this module. It has two entry
//! points, [`perform_check`] and [`install_offer`], and both are plain
//! blocking functions over [`ClientConfig`]; neither knows about Tauri, the
//! webview or the [`UpdateMachine`](crate::UpdateMachine). The crate
//! documentation lists the steps of each in order.
//!
//! # One fetch
//!
//! Every request goes through [`fetch`]. It asks the [`HostPolicy`] about the
//! URL, sends one `GET`, and follows redirects itself, at most
//! [`MAX_REDIRECTS`] of them, asking the policy again for each new URL. The
//! HTTP library is told to follow none (`redirects(0)`), because it would
//! follow them without that check. A [`Resource`] says what is being fetched
//! and with it how large the body may be, how long the request may take, and
//! whether the request names the running version and platform.
//!
//! # Verified before written
//!
//! An artifact is read into memory, checked against the digest and the
//! signature from the signed manifest, and only then written to the cache
//! directory. A download that fails either check never exists as a file, so
//! nothing unverified can be picked up from the cache, by this process or by
//! another.
//!
//! # A file already under the artifact's name
//!
//! The cache file is named after the digest in the signed manifest, so a
//! second attempt at the same release finds what the first one left: the
//! artifact a still-running or crashed installer was started from. That file
//! is not trusted for being there. It is read back and put through the same
//! two checks as a download, and only bytes that pass are handed to the
//! installer, without a second download. Any other file or link at that
//! name is replaced by the verified download: the bytes go to a new private
//! file beside it, which is then renamed over it. The rename replaces the
//! entry itself, so a planted link is removed, not written through. A
//! directory at that name is not replaced, and the install fails with
//! [`UpdateError::CacheIo`]; this crate creates none there.
//!
//! On Windows the file found there can be the installer a previous attempt
//! started, still running. It passes the checks and is started again; what
//! two runs of the installer do to each other is the installer's concern
//! and is not tested here.

use crate::artifact_limit::MAX_ARTIFACT_BYTES;
use crate::error::{FeedRefusal, Result, UpdateError};
use crate::hosts::HostPolicy;
use crate::notes::sanitize_notes;
use crate::verify::{parse_public_key, parse_sha256_hex, sha256, to_hex, verify_minisign};
use crate::version::parse_version;
use minisign_verify::PublicKey;
use semver::Version;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

/// The URL of `latest.json` in the newest published release of the source
/// repository.
///
/// GitHub resolves `releases/latest` to published, non-prerelease releases
/// only, so a draft awaiting promotion is never offered.
/// Trust is the baked minisign key, not GitHub.
pub(crate) const UPDATE_FEED_URL: &str =
    "https://github.com/ourovoros-io/oikonomia/releases/latest/download/latest.json";

/// The largest feed body that is read: 1 MiB.
///
/// The feed is held in memory to be verified. A real one is a few kilobytes
/// (one entry per platform and a line of notes), so the limit is generous
/// and still keeps a hostile server from filling memory.
pub(crate) const MAX_MANIFEST_BYTES: usize = 1_048_576;

/// The largest detached feed signature that is read: 16 KiB.
///
/// A minisign signature file is four short lines, and the Tauri signer's
/// base64 of it is about 400 bytes.
pub(crate) const MAX_SIGNATURE_BYTES: usize = 16_384;

/// The most redirects one fetch follows before it fails.
///
/// GitHub answers a release download with a redirect to its asset host,
/// and `releases/latest` adds one before it. The rest is allowance for the
/// host to add a hop, and the limit ends a redirect loop.
pub(crate) const MAX_REDIRECTS: u8 = 5;

/// The time limit of one whole feed or signature request.
///
/// It covers connecting and then everything up to the last byte of the
/// body. A redirect starts a new request with a new deadline. A slow DNS
/// lookup may exceed it: ureq cannot interrupt one (`AgentBuilder::timeout`
/// docs).
const METADATA_DEADLINE: Duration = Duration::from_secs(20);

/// The time limit for opening the connection of one artifact request.
///
/// The DNS lookup before it is not bounded, for the reason given above.
const ARTIFACT_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// The time limit of each socket read of an artifact response.
///
/// A download that stalls for this long fails; one that keeps delivering
/// bytes has no time limit, because an artifact of up to
/// [`MAX_ARTIFACT_BYTES`] can take minutes.
const ARTIFACT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// How this copy of the app receives a new version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallRoute {
    /// The app replaces itself: a macOS bundle, an `AppImage`, or a Windows
    /// per-user installer.
    InApp,
    /// The files belong to the system package manager (a `.deb` install).
    /// The app may report a newer version but must never write over itself.
    PackageManager,
}

/// What the platform installer did with a verified artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallHandoff {
    /// The new version is on disk in place of the old one; restart into it.
    Replaced,
    /// A separate installer process is running from the artifact and will
    /// replace the app once this process exits. The artifact must stay.
    InstallerStarted,
}

/// The result of [`install_offer`].
#[derive(Debug)]
pub enum InstallOutcome {
    /// The download, a check of the artifact, the cache or the installer
    /// failed, with the error it failed with; nothing was replaced.
    Failed(UpdateError),
    /// The installer ran; the caller restarts or exits as the handoff says.
    Installed(InstallHandoff),
}

/// The inputs of a check or an install.
///
/// Built once by the desktop from values compiled into it. Nothing in it
/// comes from the webview, which therefore cannot choose the feed, the hosts
/// or the key.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Where the feed is. The detached signature is at the same URL with
    /// `.sig` appended to the path.
    feed_url: Url,
    /// The updater public key that the feed and the artifact must be signed
    /// with.
    public_key: PublicKey,
    /// The running version. Only a published version greater than this is
    /// offered.
    current_version: Version,
    /// The key of this copy's entry in the feed's platform table, such as
    /// `darwin-aarch64`.
    platform: String,
    /// The time limit of one feed or signature request.
    metadata_deadline: Duration,
    /// The directory a verified artifact is written to and run from.
    cache_dir: PathBuf,
    /// The schemes and hosts that may be contacted.
    host_policy: HostPolicy,
    /// Whether this copy installs its own updates.
    install_route: InstallRoute,
}

impl ClientConfig {
    /// Builds the configuration the desktop uses.
    ///
    /// The feed URL and the host allow-list are the ones built into this
    /// crate, and the platform is the one this binary was compiled for.
    ///
    /// `public_key` is the updater minisign key, as the key file or the
    /// base64 of it. `current_version` is the running version, with or
    /// without a leading `v`. `install_route` says whether this copy may
    /// replace itself.
    ///
    /// `cache_dir` is where a verified artifact is written and then run from.
    /// The caller must pass a directory under the user's own cache location:
    /// never the vault data directory, and never a directory another account
    /// can write to, such as a shared temporary directory. The directory is
    /// created on the first install, and on Unix forced to mode `0700`.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError::InvalidFeedUrl`] when the feed constant does not
    /// parse, [`UpdateError::ArtifactUrl`] when it is not on the production
    /// allow-list, [`UpdateError::MissingPublicKey`] when `public_key` is
    /// empty or not a minisign key, and [`UpdateError::InvalidVersion`] when
    /// `current_version` is not `SemVer`.
    pub fn production(
        public_key: &str,
        current_version: &str,
        cache_dir: PathBuf,
        install_route: InstallRoute,
    ) -> Result<Self> {
        let feed_url = Url::parse(UPDATE_FEED_URL).map_err(|_| UpdateError::InvalidFeedUrl)?;
        let host_policy = HostPolicy::production();
        if !host_policy.is_allowed_fetch_url(&feed_url) {
            return Err(UpdateError::ArtifactUrl);
        }

        Ok(Self {
            feed_url,
            public_key: parse_public_key(public_key)?,
            current_version: parse_version(current_version)?,
            platform: current_updater_platform(),
            metadata_deadline: METADATA_DEADLINE,
            cache_dir,
            host_policy,
            install_route,
        })
    }
}

#[cfg(test)]
impl ClientConfig {
    /// Builds a configuration that points at a test server.
    ///
    /// Compiled for tests only: a release build has no way to set the feed
    /// URL, the host policy or the platform. The install route is
    /// [`InstallRoute::InApp`]; see [`Self::with_install_route`].
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError::MissingPublicKey`] when `public_key` is empty or
    /// not a minisign key, and [`UpdateError::InvalidVersion`] when
    /// `current_version` is not `SemVer`.
    #[expect(
        clippy::too_many_arguments,
        reason = "one argument per field a test sets; tracked for the API pass"
    )]
    pub(crate) fn for_test(
        feed_url: Url,
        public_key: &str,
        current_version: &str,
        platform: impl Into<String>,
        cache_dir: PathBuf,
        host_policy: HostPolicy,
        metadata_deadline: Duration,
    ) -> Result<Self> {
        Ok(Self {
            feed_url,
            public_key: parse_public_key(public_key)?,
            current_version: parse_version(current_version)?,
            platform: platform.into(),
            metadata_deadline,
            cache_dir,
            host_policy,
            install_route: InstallRoute::InApp,
        })
    }

    /// Returns this configuration with `install_route` in place of its own.
    pub(crate) fn with_install_route(self, install_route: InstallRoute) -> Self {
        Self {
            install_route,
            ..self
        }
    }
}

/// A newer release this copy may install, read from a manifest whose
/// detached signature verified.
///
/// The fields are private and there is no public constructor, so a value of
/// this type always comes from [`perform_check`] and cannot be altered on its
/// way to [`install_offer`].
#[derive(Debug)]
pub struct VerifiedOffer {
    /// The published version, parsed from the manifest. It is newer than the
    /// running one.
    version: Version,
    /// The release notes after [`sanitize_notes`].
    notes: String,
    /// Where the artifact is downloaded from. It passed
    /// [`HostPolicy::is_allowed_artifact_url`] when the offer was built.
    artifact_url: Url,
    /// The minisign signature over the artifact, as the manifest gives it.
    artifact_signature: String,
    /// The SHA-256 the downloaded artifact must have.
    sha256: [u8; 32],
}

impl VerifiedOffer {
    /// Returns the published version. A leading `v` in the manifest is not
    /// part of it.
    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// Returns the release notes with every HTML markup character escaped.
    #[must_use]
    pub fn notes(&self) -> &str {
        &self.notes
    }
}

/// The result of [`perform_check`]. It never carries a URL.
#[derive(Debug)]
pub enum CheckOutcome {
    /// Nothing newer is published: the signed manifest names this version or
    /// an older one, or the server answered 204 to the feed request.
    UpToDate,
    /// Newer signed manifest with an allow-listed artifact URL, for a copy
    /// that installs its own updates ([`InstallRoute::InApp`]).
    Available(VerifiedOffer),
    /// The same finding for a copy the system package manager owns
    /// ([`InstallRoute::PackageManager`]). It carries no artifact, so there
    /// is nothing an install could be started with.
    AvailableManually {
        /// The published version, parsed from the signed manifest.
        version: Version,
        /// The release notes with every HTML markup character escaped.
        notes: String,
    },
    /// The check could not be completed or its result could not be trusted,
    /// with the error it ended on. [`UpdateError::code`] tells a server that
    /// could not be reached from a feed that must not be trusted.
    Failed(UpdateError),
}

/// The platform installer, called only with an artifact whose digest and
/// signature have been verified.
pub trait ArtifactInstaller {
    /// Installs the verified artifact at `artifact`, a file in the cache
    /// directory, and says how the new version takes over.
    ///
    /// # Errors
    ///
    /// Returns an [`UpdateError`] when the artifact could not be installed.
    /// [`install_offer`] deletes the artifact and returns the error in
    /// [`InstallOutcome::Failed`].
    fn install(&self, artifact: &Path) -> Result<InstallHandoff>;
}

/// Checks the feed for a newer version. Does not download the artifact.
///
/// Blocks on the network for up to the feed deadline per request. A failure
/// is an outcome, [`CheckOutcome::Failed`], because the caller's state
/// machine has to be told of it like any other end of a check. It holds the
/// error, and the outcome is not logged here; the caller decides what to
/// record. One thing is logged in this crate: the transport error of a
/// request that got no response, which [`UpdateError::Network`] does not
/// carry and which would otherwise be lost where it is dropped.
#[must_use]
pub fn perform_check(config: &ClientConfig) -> CheckOutcome {
    perform_check_inner(config).unwrap_or_else(CheckOutcome::Failed)
}

/// Downloads and verifies the artifact of `offer`, then hands it to `installer`.
///
/// Does not touch an [`UpdateMachine`](crate::UpdateMachine), so the caller
/// need not hold one locked for the minutes a download may take. The offer
/// comes from [`UpdateMachine::begin_install`](crate::UpdateMachine::begin_install)
/// and the outcome goes to
/// [`UpdateMachine::finish_install`](crate::UpdateMachine::finish_install).
///
/// The artifact is verified in memory before it is written, so a failed
/// download or check leaves no file and `installer` is not called. The
/// artifact is deleted afterwards unless an installer process is still
/// running from it. A failure is returned in [`InstallOutcome::Failed`] with
/// its error, whether it came from this crate or from `installer`. As with
/// [`perform_check`], the outcome is not logged here; only the transport
/// error of a request that got no response is.
#[must_use]
pub fn install_offer(
    config: &ClientConfig,
    offer: &VerifiedOffer,
    installer: &impl ArtifactInstaller,
) -> InstallOutcome {
    let path = match download_and_verify(config, offer) {
        Ok(path) => path,
        Err(error) => return InstallOutcome::Failed(error),
    };

    match installer.install(&path) {
        Ok(InstallHandoff::Replaced) => {
            delete_artifact(&path);
            InstallOutcome::Installed(InstallHandoff::Replaced)
        }
        Ok(InstallHandoff::InstallerStarted) => {
            InstallOutcome::Installed(InstallHandoff::InstallerStarted)
        }
        Err(error) => {
            delete_artifact(&path);
            InstallOutcome::Failed(error)
        }
    }
}

/// Runs the checks an installed copy makes on a feed whose signature has
/// verified, for each of `platforms`, and downloads nothing.
///
/// For the release lane, before a feed is published. These are the checks
/// of a feed's content in the check sequence: the parse of step 4, the
/// version parse of step 5, and steps 6 and 7 for each platform. The version
/// is parsed and not compared, since the release lane has no running version
/// to compare it with. A feed this refuses would be refused, after
/// publication, by every copy on the named platform that is older than the
/// feed's version; a copy that is not older stops at step 5 and never reads
/// its entry. The feed is read through the client's own parser, version
/// parser and entry check, with the host allow-list the application ships
/// with, so the two cannot disagree.
///
/// `manifest` is the body of `latest.json`. Its detached signature is not
/// looked at here; [`verify_signature`](crate::verify_signature) checks
/// that. `platforms` are the feed's platform keys to check, such as
/// `linux-x86_64`.
///
/// # Examples
///
/// ```
/// use oikonomia_update::{FeedRefusal, check_feed_as_client};
///
/// let feed = br#"{
///   "version": "0.2.0",
///   "platforms": {
///     "linux-x86_64": {
///       "url": "https://example.com/Oikonomia.AppImage",
///       "signature": "signature",
///       "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
///     }
///   }
/// }"#;
///
/// let refusal = check_feed_as_client(feed, &["linux-x86_64"]).unwrap_err();
/// assert!(matches!(refusal, FeedRefusal::Entry { .. }));
/// assert_eq!(
///     refusal.to_string(),
///     "linux-x86_64: installed copies refuse https://example.com/Oikonomia.AppImage"
/// );
/// ```
///
/// # Errors
///
/// Returns [`FeedRefusal::Unreadable`] when the body is not the JSON the
/// client reads or its version is not `SemVer`,
/// [`FeedRefusal::MissingPlatform`] for the first of `platforms` the feed
/// has no entry for, and [`FeedRefusal::Entry`] for the first entry whose
/// URL does not parse, is off the allow-list or ends in `.deb`, whose
/// signature is empty, or whose SHA-256 is not 64 hex characters.
pub fn check_feed_as_client(
    manifest: &[u8],
    platforms: &[&str],
) -> std::result::Result<(), FeedRefusal> {
    let (manifest, _version) = parse_manifest(manifest).map_err(FeedRefusal::Unreadable)?;
    let host_policy = HostPolicy::production();

    for &platform in platforms {
        let Some(entry) = manifest.platforms.get(platform) else {
            return Err(FeedRefusal::MissingPlatform {
                platform: platform.to_owned(),
            });
        };

        checked_artifact(&host_policy, entry).map_err(|source| FeedRefusal::Entry {
            platform: platform.to_owned(),
            url: entry.url.clone(),
            source,
        })?;
    }

    Ok(())
}

/// Returns the key this binary looks itself up by in a feed: `{os}-{arch}`
/// in Tauri's spelling, such as `linux-x86_64` or `darwin-aarch64`.
#[must_use]
pub(crate) fn current_updater_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

/// Runs the check and returns a failure as an error, which
/// [`perform_check`] turns into [`CheckOutcome::Failed`].
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactUrl`] when the feed URL or a redirect from
/// it is off the allow-list, or the artifact URL in the manifest does not
/// parse, is off the allow-list or ends in `.deb`;
/// [`UpdateError::Network`] when the feed or its signature cannot be fetched
/// or exceeds its size limit;
/// [`UpdateError::ManifestSignature`] when the signature is absent or does
/// not verify, or the manifest's artifact signature is empty;
/// [`UpdateError::ManifestParse`] when the signed body is not the expected
/// JSON; [`UpdateError::InvalidVersion`] when its version is not `SemVer`;
/// [`UpdateError::MissingPlatform`] when a newer version lists no artifact
/// for this platform; and [`UpdateError::ArtifactIntegrity`] when the
/// manifest's SHA-256 is not 64 hex characters.
pub(crate) fn perform_check_inner(config: &ClientConfig) -> Result<CheckOutcome> {
    let Fetched::Body(body) = fetch(config, &config.feed_url, Resource::Manifest)? else {
        return Ok(CheckOutcome::UpToDate);
    };

    // Derived from the configured URL, not from the URL that was requested:
    // `fetch` appends the identity query itself, once per request.
    let signature_url = signature_url_for(&config.feed_url);
    let Fetched::Body(signature) = fetch(config, &signature_url, Resource::ManifestSignature)?
    else {
        return Err(UpdateError::ManifestSignature);
    };
    let signature = std::str::from_utf8(&signature).map_err(|_| UpdateError::ManifestSignature)?;

    // Over the bytes as they arrived, and before anything reads them: the
    // JSON parser never sees a body the key holder did not sign.
    verify_minisign(&config.public_key, &body, signature)?;

    // The version is compared before the platform entry is read: a copy that
    // is already current has no use for an artifact, so a feed that lists
    // none for its platform is not a failure for it.
    let (manifest, remote) = parse_manifest(&body)?;
    if remote <= config.current_version {
        return Ok(CheckOutcome::UpToDate);
    }

    // The offer is built, and so its artifact entry checked, on both
    // routes: a feed this copy could not install from is a failed check for
    // a package-managed copy too.
    let offer = offer_from_manifest(config, &manifest, remote)?;
    Ok(match config.install_route {
        InstallRoute::InApp => CheckOutcome::Available(offer),
        InstallRoute::PackageManager => CheckOutcome::AvailableManually {
            version: offer.version,
            notes: offer.notes,
        },
    })
}

/// Puts the verified artifact of `offer` in the cache directory and returns
/// the path of the file.
///
/// A file an earlier attempt left under the artifact's name is used as it is
/// when its bytes pass the digest and signature checks; nothing is
/// downloaded then. Otherwise the artifact is downloaded, checked, and
/// written in place of the file or link at that name.
///
/// Downloaded bytes are held in memory until both checks pass and only then
/// written, so a failed download or a failed check leaves no file: a
/// leftover that was not the artifact is removed with it, and so is a file
/// left by a write that failed part-way.
///
/// # Errors
///
/// Returns [`UpdateError::CacheIo`] when the cache directory cannot be
/// created or made private, or the file cannot be written or moved to its
/// name, as when a directory is at that name;
/// [`UpdateError::ArtifactUrl`] when the artifact URL, or a redirect from it,
/// is off the allow-list; [`UpdateError::Network`] when the download fails or
/// the server answers 204; [`UpdateError::ArtifactTooLarge`] when the
/// artifact exceeds [`MAX_ARTIFACT_BYTES`]; and
/// [`UpdateError::ArtifactIntegrity`] when the digest or the signature of
/// the download does not match.
pub(crate) fn download_and_verify(config: &ClientConfig, offer: &VerifiedOffer) -> Result<PathBuf> {
    prepare_cache_dir(&config.cache_dir).map_err(UpdateError::CacheIo)?;

    // The digest makes the name unique to these exact bytes; the URL's file
    // name follows it for the sake of its extension.
    let destination = config.cache_dir.join(format!(
        "{}-{}",
        to_hex(&offer.sha256),
        artifact_file_name(&offer.artifact_url)
    ));
    purge_cache(&config.cache_dir, &destination);

    if holds_verified_artifact(config, offer, &destination) {
        return Ok(destination);
    }

    match download_and_verify_into(config, offer, &destination) {
        Ok(()) => Ok(destination),
        Err(error) => {
            delete_artifact(&destination);
            Err(error)
        }
    }
}

/// Removes the files earlier installs left in `cache_dir`, except `keep`.
///
/// `keep` is the path the artifact of the install in flight will have. What
/// an earlier attempt at the same release left there is not removed here:
/// [`download_and_verify`] reuses it when it is the artifact and replaces it
/// when it is not.
///
/// Best effort: a file that cannot be removed, such as an installer that is
/// still running on Windows, stays until a later install clears it. Every
/// failure is logged and none is returned. Such a leftover is of another
/// release and does not stand in the way of the install that follows, whose
/// file name starts with its own digest.
///
/// Symbolic links are removed too, never followed: `file_type` describes the
/// entry itself. Directories are left alone; this crate creates none here.
pub(crate) fn purge_cache(cache_dir: &Path, keep: &Path) {
    let entries = match std::fs::read_dir(cache_dir) {
        Ok(entries) => entries,
        Err(error) => {
            log::warn!(
                "updater cache could not be listed at {}: {error}",
                cache_dir.display()
            );
            return;
        }
    };

    for entry in entries {
        match entry {
            Ok(entry) => {
                let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
                let path = entry.path();
                if !is_directory && path != keep {
                    delete_artifact(&path);
                }
            }
            Err(error) => {
                log::warn!(
                    "updater cache entry could not be read in {}: {error}",
                    cache_dir.display()
                );
            }
        }
    }
}

/// Removes the artifact at `path`. Used after a failed download or install,
/// and once an installer has replaced the app.
///
/// A file that is already gone is not a failure. Any other failure is logged
/// and not returned: this is cleanup, and it must not replace the outcome of
/// the step it follows.
pub(crate) fn delete_artifact(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            log::warn!(
                "updater artifact removal failed for {}: {error}",
                path.display()
            );
        }
    }
}

/// Writes `bytes` to a file that must not exist yet.
///
/// `create_new` refuses an existing path, a symbolic link included, so the
/// write cannot be redirected to a file outside the cache. On Unix the file
/// is created with mode `0600`.
///
/// # Errors
///
/// Returns the I/O error of creating, writing or syncing the file; its kind
/// is `AlreadyExists` when something is at `path`.
pub(crate) fn write_new_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// The feed as the client reads it. Fields the client has no use for are
/// ignored.
#[derive(Debug, Deserialize)]
struct RawManifest {
    /// The published version, with or without a leading `v`.
    version: String,
    /// The release notes, unsanitized. Absent in a feed without notes.
    #[serde(default)]
    notes: Option<String>,
    /// The artifact of each platform, by platform key. A feed without the
    /// table reads as one that offers no platform.
    #[serde(default)]
    platforms: HashMap<String, RawPlatform>,
}

/// One platform's artifact as the feed describes it.
#[derive(Debug, Deserialize)]
struct RawPlatform {
    /// Where the artifact is downloaded from.
    url: String,
    /// The minisign signature over the artifact.
    signature: String,
    /// The hex SHA-256 of the artifact.
    sha256: String,
}

/// The artifact of one platform entry, after the checks a copy makes before
/// it offers the entry.
struct CheckedArtifact {
    /// Where the artifact is downloaded from. It passed
    /// [`HostPolicy::is_allowed_artifact_url`].
    url: Url,
    /// The minisign signature over the artifact. It is not empty; whether
    /// it verifies is known only once the artifact is there.
    signature: String,
    /// The SHA-256 the artifact must have.
    sha256: [u8; 32],
}

/// What a fetch is for. That fixes its size cap, its time limits, and whether
/// the request tells the server which copy of the app is asking.
#[derive(Debug, Clone, Copy)]
enum Resource {
    /// The `latest.json` feed.
    Manifest,
    /// The detached minisign signature over the feed.
    ManifestSignature,
    /// The platform artifact a verified manifest names.
    Artifact,
}

impl Resource {
    /// Returns the largest body that is read for this resource.
    fn max_bytes(self) -> usize {
        match self {
            Self::Manifest => MAX_MANIFEST_BYTES,
            Self::ManifestSignature => MAX_SIGNATURE_BYTES,
            Self::Artifact => MAX_ARTIFACT_BYTES,
        }
    }

    /// Returns whether the request carries the running version and platform.
    ///
    /// Only the feed endpoints are told, which is what lets a feed server
    /// answer 204 to a copy that is current. The artifact URL comes from the
    /// manifest and is fetched exactly as signed.
    fn names_this_copy(self) -> bool {
        match self {
            Self::Manifest | Self::ManifestSignature => true,
            Self::Artifact => false,
        }
    }
}

/// What a fetch that reached its last hop came back with.
enum Fetched {
    /// HTTP 200 and its body, within the size cap of the resource.
    Body(Vec<u8>),
    /// HTTP 204.
    NoContent,
}

/// Why a fetch ended without a body or a 204.
enum FetchFailure {
    /// No response arrived, a redirect could not be followed, or the body
    /// could not be read to its end.
    Network,
    /// The last hop answered with this status, which is not 200, 204 or a
    /// redirect.
    Status(u16),
    /// A URL on the way was off the allow-list.
    Denied,
    /// The body was larger than the cap of the resource.
    TooLarge,
}

/// Parses a feed body, and the version it states, the way every copy does.
///
/// The caller has verified the detached signature over `body`, or is the
/// release lane asking what a copy would make of it.
///
/// # Errors
///
/// Returns [`UpdateError::ManifestParse`] when `body` is not the expected
/// JSON, and [`UpdateError::InvalidVersion`] when its version is not `SemVer`.
fn parse_manifest(body: &[u8]) -> Result<(RawManifest, Version)> {
    let manifest: RawManifest =
        serde_json::from_slice(body).map_err(|_| UpdateError::ManifestParse)?;
    let version = parse_version(&manifest.version)?;

    Ok((manifest, version))
}

/// Builds the offer for `version` from this platform's entry in `manifest`.
///
/// # Errors
///
/// Returns [`UpdateError::MissingPlatform`] when the manifest has no entry
/// for the configured platform, and otherwise what [`checked_artifact`]
/// returns for the entry.
fn offer_from_manifest(
    config: &ClientConfig,
    manifest: &RawManifest,
    version: Version,
) -> Result<VerifiedOffer> {
    let entry = manifest
        .platforms
        .get(&config.platform)
        .ok_or(UpdateError::MissingPlatform)?;
    let artifact = checked_artifact(&config.host_policy, entry)?;
    let notes = sanitize_notes(manifest.notes.as_deref().unwrap_or(""));

    Ok(VerifiedOffer {
        version,
        notes,
        artifact_url: artifact.url,
        artifact_signature: artifact.signature,
        sha256: artifact.sha256,
    })
}

/// Checks one platform entry of a feed against `host_policy` and returns its
/// artifact.
///
/// Both the client and [`check_feed_as_client`] decide through this
/// function whether an entry may be offered.
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactUrl`] when the entry's URL does not parse,
/// is off the allow-list or ends in `.deb`;
/// [`UpdateError::ManifestSignature`] when its signature is empty; and
/// [`UpdateError::ArtifactIntegrity`] when its SHA-256 is not 64 hex
/// characters.
fn checked_artifact(host_policy: &HostPolicy, entry: &RawPlatform) -> Result<CheckedArtifact> {
    let url = Url::parse(&entry.url).map_err(|_| UpdateError::ArtifactUrl)?;
    if !host_policy.is_allowed_artifact_url(&url) {
        return Err(UpdateError::ArtifactUrl);
    }
    if entry.signature.trim().is_empty() {
        return Err(UpdateError::ManifestSignature);
    }
    let sha256 = parse_sha256_hex(&entry.sha256)?;

    Ok(CheckedArtifact {
        url,
        signature: entry.signature.clone(),
        sha256,
    })
}

/// Returns whether the file at `path` is the verified artifact of `offer`
/// already, so that it can be handed to the installer as it is.
///
/// The file is trusted for nothing but its bytes passing the checks a
/// download passes. It must also be a file this crate could have written: a
/// regular file, not a link, that on Unix belongs to the owner of the cache
/// directory and grants nothing to group or others. A link is refused even
/// to the right bytes, because the installer is given the path, and what a
/// link points at can change after the check.
///
/// Every reason the file cannot be used reads as `false`, a failure to read
/// it included: the caller then downloads the artifact and replaces the
/// file, and a real fault of the cache shows there as an error.
fn holds_verified_artifact(config: &ClientConfig, offer: &VerifiedOffer, path: &Path) -> bool {
    read_private_file(path, MAX_ARTIFACT_BYTES)
        .is_some_and(|bytes| verify_artifact(config, offer, &bytes).is_ok())
}

/// Reads the file at `path` when it is a regular file of at most `max_bytes`
/// that, on Unix, belongs to the owner of the directory it is in and grants
/// nothing to group or others.
///
/// Returns `None` when there is no such file, when it is anything else (a
/// link, a directory, a larger file, one of another owner or one more widely
/// permitted), or when it cannot be read.
///
/// The kind, the owner and the mode are those of the entry itself, never of
/// a link's target. They are read before the file is opened, so they
/// describe the file that is read only as long as nobody else can change the
/// directory in between; the cache directory is the user's alone for that
/// reason.
fn read_private_file(path: &Path, max_bytes: usize) -> Option<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    let directory = std::fs::metadata(path.parent()?).ok()?;
    if !metadata.file_type().is_file() || !is_private_to_owner_of(&metadata, &directory) {
        return None;
    }

    // The size is asked first so that a file that is too large is not read
    // at all, and counted again while reading in case it grows. One byte
    // past the limit is enough to tell.
    let max_length = u64::try_from(max_bytes).unwrap_or(u64::MAX);
    if metadata.len() > max_length {
        return None;
    }
    let limit = max_length.saturating_add(1);
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(limit)
        .read_to_end(&mut bytes)
        .ok()?;

    (bytes.len() <= max_bytes).then_some(bytes)
}

/// Returns whether the file `metadata` describes belongs to the owner of
/// the directory `directory` describes and leaves group and others with no
/// access.
///
/// The mode alone would not do: a file of another account at mode `0600`
/// can be rewritten by that account after it has been verified. The cache
/// directory is the reference because its mode was set a moment ago, which
/// the system allows only its owner, or root.
#[cfg(unix)]
#[expect(
    clippy::verbose_bit_mask,
    reason = "the mask is the group and other permission bits, as chmod writes them"
)]
fn is_private_to_owner_of(metadata: &std::fs::Metadata, directory: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.uid() == directory.uid() && metadata.mode() & 0o077 == 0
}

/// Returns true: there are no Unix owner and mode to read here, and the
/// caller's choice of cache directory is what keeps other accounts out.
#[cfg(not(unix))]
fn is_private_to_owner_of(_metadata: &std::fs::Metadata, _directory: &std::fs::Metadata) -> bool {
    true
}

/// Checks `bytes` against the digest and the artifact signature of `offer`.
///
/// The one check of an artifact, whether its bytes came from the network or
/// from a file an earlier attempt left in the cache.
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactIntegrity`] when the SHA-256 of `bytes` is
/// not the offered one or the signature does not verify them.
fn verify_artifact(config: &ClientConfig, offer: &VerifiedOffer, bytes: &[u8]) -> Result<()> {
    if sha256(bytes) != offer.sha256 {
        return Err(UpdateError::ArtifactIntegrity);
    }

    verify_minisign(&config.public_key, bytes, &offer.artifact_signature)
        .map_err(|_| UpdateError::ArtifactIntegrity)
}

/// Writes `bytes` to `destination` in place of the file or link there.
///
/// The bytes go to a new private file beside `destination`, which is then
/// renamed to it. A file an earlier attempt left at `destination` is
/// replaced, and so is a planted symbolic link: the rename replaces the
/// directory entry and never writes through it. The file at `destination` is
/// therefore always one [`write_new_private_file`] created. A directory at
/// `destination` is not replaced; the rename fails.
///
/// The staging name carries the process id, which keeps two running copies
/// apart. Whatever is already under that name, left by an earlier attempt,
/// is removed first; if it cannot be, the write fails. The staging file is
/// removed when the write or the rename fails.
///
/// # Errors
///
/// Returns the I/O error of creating, writing or syncing the staging file, or
/// of the rename. Whether the system replaces a `destination` that is in
/// use, as a running installer's own file is on Windows, is the system's
/// decision and is not tested here; its refusal is this error.
fn replace_with_private_file(destination: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let staging = {
        let mut name = destination.as_os_str().to_owned();
        name.push(format!(".{}.partial", std::process::id()));
        PathBuf::from(name)
    };
    delete_artifact(&staging);

    let replaced = write_new_private_file(&staging, bytes)
        .and_then(|()| std::fs::rename(&staging, destination));
    if replaced.is_err() {
        delete_artifact(&staging);
    }

    replaced
}

/// Downloads the artifact of `offer`, verifies it, and writes it to
/// `destination`, replacing what is there.
///
/// # Errors
///
/// As [`download_and_verify`], except for the cache directory, which the
/// caller has prepared.
fn download_and_verify_into(
    config: &ClientConfig,
    offer: &VerifiedOffer,
    destination: &Path,
) -> Result<()> {
    // Checked when the offer was built, and again here: the offer may have
    // waited, and this is the last step before the request.
    if !config
        .host_policy
        .is_allowed_artifact_url(&offer.artifact_url)
    {
        return Err(UpdateError::ArtifactUrl);
    }
    let Fetched::Body(bytes) = fetch(config, &offer.artifact_url, Resource::Artifact)? else {
        return Err(UpdateError::Network);
    };

    verify_artifact(config, offer, &bytes)?;

    replace_with_private_file(destination, &bytes).map_err(UpdateError::CacheIo)
}

/// Creates the cache directory for this user only.
///
/// The verified artifact is read back from here by path and then run, so
/// nobody else may be able to swap it in between. On Unix the directory is
/// forced to mode `0700`; that fails, and the install with it, when the
/// directory belongs to another account. The caller passes a directory under
/// the user's own cache location, never a shared temporary directory.
///
/// # Errors
///
/// Returns the I/O error of creating the directory or setting its mode.
fn prepare_cache_dir(cache_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_dir)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(cache_dir, std::fs::Permissions::from_mode(0o700))?;
    }

    Ok(())
}

/// Returns the local file name for a downloaded artifact: the last segment
/// of its URL, reduced to ASCII letters, digits, `.`, `-` and `_`.
///
/// The extension has to survive the download. The installers choose their
/// action from it, and Windows will not start a program whose name has no
/// extension. Every other character becomes `_`, so the name holds no path
/// separator and cannot leave the cache directory.
fn artifact_file_name(url: &Url) -> String {
    /// Longest name kept. With the 64-character digest and the hyphen put
    /// in front of it, the file name stays well under the 255 bytes common
    /// file systems allow.
    const MAX_NAME_BYTES: usize = 96;

    let segment = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or("");

    let mut name: String = segment
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();

    // Keep the tail: that is where the extension lives. The name is ASCII by
    // now, so every byte offset is a character boundary.
    let excess = name.len().saturating_sub(MAX_NAME_BYTES);
    let name = name.split_off(excess);
    let name = name.trim_start_matches('.');

    if name.is_empty() {
        "artifact".to_owned()
    } else {
        name.to_owned()
    }
}

/// Returns the URL of the detached signature of the feed at `feed`: the same
/// URL with `.sig` appended to its path.
fn signature_url_for(feed: &Url) -> Url {
    let mut signature = feed.clone();
    signature.set_path(&format!("{}.sig", feed.path()));
    signature
}

/// Fetches `url` as `resource` and returns the body, or that there was none.
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactUrl`] when `url` or a redirect from it is
/// off the allow-list; [`UpdateError::ArtifactTooLarge`] when `resource` is
/// the artifact and the body exceeds its cap;
/// [`UpdateError::ManifestSignature`] when `resource` is the feed signature
/// and the server answers 404; and [`UpdateError::Network`] for every other
/// failure, a feed or feed signature over its cap included.
fn fetch(config: &ClientConfig, url: &Url, resource: Resource) -> Result<Fetched> {
    let mut request_url = url.clone();
    if resource.names_this_copy() {
        append_identity_query(&mut request_url, config);
    }

    fetch_following_redirects(config, request_url, resource).map_err(|failure| {
        match (failure, resource) {
            (FetchFailure::Denied, _) => UpdateError::ArtifactUrl,
            // Retrying cannot help, so the user must not be told to check the
            // connection.
            (FetchFailure::TooLarge, Resource::Artifact) => UpdateError::ArtifactTooLarge,
            // A feed published without its signature is a feed that cannot be
            // trusted, which is a different finding from a server in trouble.
            (FetchFailure::Status(404), Resource::ManifestSignature) => {
                UpdateError::ManifestSignature
            }
            // An oversized manifest or signature is a broken feed and is
            // reported like one that could not be fetched.
            (FetchFailure::TooLarge, Resource::Manifest | Resource::ManifestSignature)
            | (FetchFailure::Status(_) | FetchFailure::Network, _) => UpdateError::Network,
        }
    })
}

/// Requests `url` and follows up to [`MAX_REDIRECTS`] redirects, checking
/// every URL against the host policy before it is requested.
fn fetch_following_redirects(
    config: &ClientConfig,
    mut url: Url,
    resource: Resource,
) -> std::result::Result<Fetched, FetchFailure> {
    let agent = agent_for(config, resource);

    let mut redirects_followed = 0_u8;
    loop {
        if !config.host_policy.is_allowed_fetch_url(&url) {
            return Err(FetchFailure::Denied);
        }

        // With `redirects(0)` ureq 2 hands a 3xx back as `Ok` (`connect` in
        // its `unit.rs`) and reports only a status of 400 or above as
        // `Error::Status` (the docs of that variant), so redirects are
        // followed here, one policy check per hop.
        let response = agent.get(url.as_str()).call().map_err(|error| {
            log::warn!("update fetch failed: {error}");
            match error {
                ureq::Error::Status(status, _) => FetchFailure::Status(status),
                ureq::Error::Transport(_) => FetchFailure::Network,
            }
        })?;

        match response.status() {
            200 => {
                let body = read_capped(response, resource.max_bytes())?;
                return Ok(Fetched::Body(body));
            }
            204 => return Ok(Fetched::NoContent),
            status if is_redirect(status) => {
                if redirects_followed == MAX_REDIRECTS {
                    return Err(FetchFailure::Network);
                }
                redirects_followed += 1;

                let Some(location) = response.header("Location") else {
                    return Err(FetchFailure::Network);
                };
                // A relative `Location` is resolved against the URL that
                // sent it, as a browser does.
                url = url.join(location).map_err(|_| FetchFailure::Network)?;
            }
            status => return Err(FetchFailure::Status(status)),
        }
    }
}

/// Builds the agent for one fetch, with the time limits `resource` calls for.
fn agent_for(config: &ClientConfig, resource: Resource) -> ureq::Agent {
    let builder = ureq::AgentBuilder::new()
        .redirects(0)
        .user_agent(&format!("Oikonomia/{}", config.current_version));

    let builder = match resource {
        // `timeout` is ureq's overall deadline, but `timeout_connect` takes
        // precedence over it and defaults to 30 seconds (`AgentBuilder`
        // docs), so the connect phase is bounded by the same value.
        Resource::Manifest | Resource::ManifestSignature => builder
            .timeout_connect(config.metadata_deadline)
            .timeout(config.metadata_deadline),
        // No overall deadline: it would also cap how long reading the body
        // may take, and the artifact is far larger than the feed.
        Resource::Artifact => builder
            .timeout_connect(ARTIFACT_CONNECT_TIMEOUT)
            .timeout_read(ARTIFACT_READ_TIMEOUT),
    };

    builder.build()
}

/// Returns whether `status` is a redirect this client follows.
///
/// 300 and 304 are left out: neither names one new location to request.
fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Appends the running version and the platform to the query of `url`, as
/// `version`, `os` and `arch`.
fn append_identity_query(url: &mut Url, config: &ClientConfig) {
    let (os, arch) = config
        .platform
        .split_once('-')
        .unwrap_or((config.platform.as_str(), ""));

    url.query_pairs_mut()
        .append_pair("version", &config.current_version.to_string())
        .append_pair("os", os)
        .append_pair("arch", arch);
}

/// Reads the body of `response`, giving up as soon as it is known to be
/// larger than `max_bytes`.
///
/// The size is counted as the body arrives and not taken from
/// `Content-Length`, which a server can omit or misstate.
fn read_capped(
    response: ureq::Response,
    max_bytes: usize,
) -> std::result::Result<Vec<u8>, FetchFailure> {
    let mut reader = response.into_reader();
    let mut body = Vec::new();
    let mut chunk = [0_u8; 8192];

    loop {
        let count = match reader.read(&mut chunk) {
            Ok(0) => return Ok(body),
            Ok(count) => count,
            Err(_) => return Err(FetchFailure::Network),
        };
        if body.len().saturating_add(count) > max_bytes {
            return Err(FetchFailure::TooLarge);
        }
        body.extend_from_slice(&chunk[..count]);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        check_feed_as_client, read_private_file, replace_with_private_file, write_new_private_file,
    };
    use crate::error::{FeedRefusal, UpdateError};
    use crate::feed::{FeedArtifact, assemble_manifest};

    /// A feed with one entry per name in `file_names`, keyed `platform-0`,
    /// `platform-1` and so on, under a production release URL.
    fn feed_naming(file_names: &[&str]) -> String {
        let artifacts: Vec<FeedArtifact> = file_names
            .iter()
            .enumerate()
            .map(|(index, file_name)| FeedArtifact {
                platform: format!("platform-{index}"),
                file_name: (*file_name).to_owned(),
                signature: "signature".to_owned(),
                sha256_hex: "ab".repeat(32),
            })
            .collect();
        let base_url = "https://github.com/ourovoros-io/oikonomia/releases/download/v0.2.0";

        assemble_manifest("v0.2.0", "notes", base_url, &artifacts).expect("assemble")
    }

    #[test]
    fn a_feed_the_release_lane_assembles_for_github_passes_the_client_checks() {
        let feed = feed_naming(&["Oikonomia.app.tar.gz", "Oikonomia.AppImage"]);

        let checked = check_feed_as_client(feed.as_bytes(), &["platform-0", "platform-1"]);

        assert!(checked.is_ok(), "{checked:?}");
    }

    #[test]
    fn the_client_checks_name_the_platform_a_feed_lacks() {
        let feed = feed_naming(&["Oikonomia.AppImage"]);

        let refusal = check_feed_as_client(feed.as_bytes(), &["platform-0", "windows-x86_64"])
            .expect_err("no windows entry");

        assert_eq!(
            refusal.to_string(),
            "windows-x86_64: the feed has no entry for this platform"
        );
    }

    #[test]
    fn the_client_checks_refuse_a_debian_package_as_an_in_app_artifact() {
        let feed = feed_naming(&["Oikonomia.AppImage", "oikonomia.deb"]);

        let refusal = check_feed_as_client(feed.as_bytes(), &["platform-0", "platform-1"])
            .expect_err("a .deb entry");

        assert!(
            matches!(
                &refusal,
                FeedRefusal::Entry { platform, url, source: UpdateError::ArtifactUrl }
                    if platform == "platform-1" && url.ends_with("/oikonomia.deb")
            ),
            "{refusal:?}"
        );
    }

    #[test]
    fn the_client_checks_refuse_an_entry_without_a_signature_or_a_digest() {
        let entry = |signature: &str, sha256: &str| {
            format!(
                r#"{{"version":"0.2.0","platforms":{{"linux-x86_64":{{
                    "url":"https://github.com/o/r/releases/download/v0.2.0/a.AppImage",
                    "signature":"{signature}","sha256":"{sha256}"}}}}}}"#
            )
        };

        let unsigned =
            check_feed_as_client(entry(" ", &"ab".repeat(32)).as_bytes(), &["linux-x86_64"])
                .expect_err("empty signature");
        let undigested =
            check_feed_as_client(entry("signature", "abc").as_bytes(), &["linux-x86_64"])
                .expect_err("short digest");

        assert!(
            matches!(
                unsigned,
                FeedRefusal::Entry {
                    source: UpdateError::ManifestSignature,
                    ..
                }
            ),
            "{unsigned:?}"
        );
        assert!(
            matches!(
                undigested,
                FeedRefusal::Entry {
                    source: UpdateError::ArtifactIntegrity,
                    ..
                }
            ),
            "{undigested:?}"
        );
    }

    #[test]
    fn a_private_file_is_read_back_only_within_the_size_limit() {
        let cache = tempfile::tempdir().expect("temporary directory");
        let path = cache.path().join("artifact.AppImage");
        write_new_private_file(&path, b"four").expect("file");

        assert_eq!(read_private_file(&path, 4), Some(b"four".to_vec()));
        assert_eq!(read_private_file(&path, 3), None);
    }

    #[test]
    fn a_directory_or_a_missing_path_is_not_read_back_as_a_file() {
        let cache = tempfile::tempdir().expect("temporary directory");
        let directory = cache.path().join("a-directory");
        std::fs::create_dir(&directory).expect("directory");

        assert_eq!(read_private_file(&directory, 16), None);
        assert_eq!(read_private_file(&cache.path().join("absent"), 16), None);
    }

    #[test]
    fn replacing_clears_a_staging_file_an_earlier_attempt_left() {
        let cache = tempfile::tempdir().expect("temporary directory");
        let path = cache.path().join("artifact.AppImage");
        let staging = cache
            .path()
            .join(format!("artifact.AppImage.{}.partial", std::process::id()));
        std::fs::write(&staging, b"half written").expect("stale staging file");

        replace_with_private_file(&path, b"verified").expect("replace");

        assert_eq!(std::fs::read(&path).expect("read"), b"verified");
        assert!(!staging.exists());
    }

    #[test]
    fn the_client_checks_refuse_a_body_that_is_not_a_feed() {
        let refusal = check_feed_as_client(b"not json", &[]).expect_err("not a feed");

        assert!(
            matches!(refusal, FeedRefusal::Unreadable(UpdateError::ManifestParse)),
            "{refusal:?}"
        );
    }

    #[test]
    fn artifact_write_refuses_a_path_that_already_exists() {
        let cache = tempfile::tempdir().expect("temporary directory");
        let path = cache.path().join("artifact.AppImage");
        std::fs::write(&path, b"planted").expect("existing file");

        let err = write_new_private_file(&path, b"verified").expect_err("exists");

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).expect("read"), b"planted");
    }

    #[cfg(unix)]
    #[test]
    fn artifact_write_does_not_follow_a_link_at_its_path() {
        let cache = tempfile::tempdir().expect("temporary directory");
        let outside_dir = tempfile::tempdir().expect("temporary directory");
        let outside = outside_dir.path().join("victim");
        let path = cache.path().join("artifact.AppImage");
        std::os::unix::fs::symlink(&outside, &path).expect("plant link");

        let err = write_new_private_file(&path, b"verified").expect_err("link");

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(!outside.exists(), "the write followed the planted link");
    }

    #[test]
    fn replacing_overwrites_a_file_left_at_the_path_and_leaves_no_staging_file() {
        let cache = tempfile::tempdir().expect("temporary directory");
        let path = cache.path().join("artifact.AppImage");
        std::fs::write(&path, b"left by an earlier attempt").expect("existing file");

        replace_with_private_file(&path, b"verified").expect("replace");

        assert_eq!(std::fs::read(&path).expect("read"), b"verified");
        let entries: Vec<_> = std::fs::read_dir(cache.path())
            .expect("list")
            .map(|entry| entry.expect("entry").path())
            .collect();
        assert_eq!(entries, vec![path]);
    }

    #[cfg(unix)]
    #[test]
    fn replacing_removes_a_link_at_the_path_without_writing_through_it() {
        use std::os::unix::fs::PermissionsExt;

        let cache = tempfile::tempdir().expect("temporary directory");
        let outside_dir = tempfile::tempdir().expect("temporary directory");
        let outside = outside_dir.path().join("victim");
        std::fs::write(&outside, b"untouched").expect("victim");
        let path = cache.path().join("artifact.AppImage");
        std::os::unix::fs::symlink(&outside, &path).expect("plant link");

        replace_with_private_file(&path, b"verified").expect("replace");

        let metadata = std::fs::symlink_metadata(&path).expect("metadata");
        assert!(metadata.file_type().is_file(), "the link was kept");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read(&path).expect("read"), b"verified");
        assert_eq!(std::fs::read(&outside).expect("victim"), b"untouched");
    }

    #[test]
    fn a_replacement_that_cannot_take_its_place_leaves_no_staging_file() {
        let cache = tempfile::tempdir().expect("temporary directory");
        // A file cannot be renamed over a directory that is not empty.
        let path = cache.path().join("artifact.AppImage");
        std::fs::create_dir(&path).expect("directory");
        std::fs::write(path.join("inside"), b"x").expect("file inside");

        replace_with_private_file(&path, b"verified").expect_err("a directory is in the way");

        let entries: Vec<_> = std::fs::read_dir(cache.path())
            .expect("list")
            .map(|entry| entry.expect("entry").path())
            .collect();
        assert_eq!(entries, vec![path]);
    }
}

#[cfg(test)]
mod properties {
    use super::RawManifest;
    use crate::feed::{FeedArtifact, assemble_manifest};
    use oikonomia_test_support::PROPERTY_CASES;
    use proptest::prelude::*;

    fn artifact() -> impl Strategy<Value = FeedArtifact> {
        (
            "[a-z0-9-]{1,12}",
            "[A-Za-z0-9._-]{1,20}",
            "[A-Za-z0-9+/=]{1,40}",
            "[0-9a-fA-F]{64}",
        )
            .prop_map(
                |(platform, file_name, signature, sha256_hex)| FeedArtifact {
                    platform,
                    file_name,
                    signature,
                    sha256_hex,
                },
            )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPERTY_CASES))]

        // The release lane writes the manifest and this client reads it; any
        // notes text must survive the trip, whatever JSON has to escape.
        #[test]
        fn an_assembled_manifest_parses_as_the_manifest_the_client_reads(
            // No leading zeros: `SemVer` forbids them in a numeric part.
            version in "v?(0|[1-9][0-9]{0,2})\\.(0|[1-9][0-9]{0,2})\\.(0|[1-9][0-9]{0,2})",
            notes in any::<String>(),
            artifacts in prop::collection::vec(artifact(), 1..4),
        ) {
            let base = "https://example.test/releases/";
            let body = assemble_manifest(&version, &notes, base, &artifacts);
            prop_assert!(body.is_ok(), "{:?}", body);

            let parsed = serde_json::from_str::<RawManifest>(&body.unwrap());
            prop_assert!(parsed.is_ok(), "{:?}", parsed);
            let manifest = parsed.unwrap();

            prop_assert_eq!(manifest.version.as_str(), version.trim_start_matches('v'));
            prop_assert_eq!(manifest.notes, Some(notes));
            for artifact in &artifacts {
                let platform = manifest.platforms.get(&artifact.platform);
                prop_assert!(platform.is_some(), "{} is missing", artifact.platform);
            }
        }
    }
}
