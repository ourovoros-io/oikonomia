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

/// The largest artifact that is downloaded: 200 MiB.
///
/// The artifact is held in memory until its digest and signature are checked,
/// so this is also the most memory an install takes. It has to stay above the
/// size of the largest installer the release workflow builds.
pub(crate) const MAX_ARTIFACT_BYTES: usize = 200 * 1024 * 1024;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Download, verification, or the installer failed; nothing was replaced.
    Failed,
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
    /// The check could not be completed or its result could not be trusted.
    /// The cause, an [`UpdateError`], is logged and not returned.
    Failed,
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
    /// [`install_offer`] logs it, deletes the artifact and reports
    /// [`InstallOutcome::Failed`].
    fn install(&self, artifact: &Path) -> Result<InstallHandoff>;
}

/// Checks the feed for a newer version. Does not download the artifact.
///
/// Blocks on the network for up to the feed deadline per request. The cause
/// of a failure is logged and the outcome is [`CheckOutcome::Failed`].
#[must_use]
pub fn perform_check(config: &ClientConfig) -> CheckOutcome {
    match perform_check_inner(config) {
        Ok(outcome) => outcome,
        Err(error) => {
            log_failure("update check failed", &error);
            CheckOutcome::Failed
        }
    }
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
/// running from it. The cause of a failure is logged.
#[must_use]
pub fn install_offer(
    config: &ClientConfig,
    offer: &VerifiedOffer,
    installer: &impl ArtifactInstaller,
) -> InstallOutcome {
    let path = match download_and_verify(config, offer) {
        Ok(path) => path,
        Err(error) => {
            log_failure("update install verify failed", &error);
            return InstallOutcome::Failed;
        }
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
            log_failure("update install exec failed", &error);
            delete_artifact(&path);
            InstallOutcome::Failed
        }
    }
}

/// Runs the checks an installed copy makes on a feed whose signature has
/// verified, for each of `platforms`, and downloads nothing.
///
/// For the release lane, before a feed is published: a feed this accepts is
/// one a copy on each of those platforms accepts at steps 4 to 7 of the
/// check, and one it refuses would be refused by every such copy after
/// publication. It reads the feed through the client's own parser, version
/// rule and entry checks, with the host allow-list the application ships
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

/// Runs the check and keeps the cause of a failure, which [`perform_check`]
/// logs and reduces to [`CheckOutcome::Failed`].
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactUrl`] when the feed URL or a redirect from
/// it is off the allow-list, or the artifact URL in the manifest does not
/// parse, is off the allow-list or ends in `.deb`;
/// [`UpdateError::Network`] when the feed or its signature cannot be fetched;
/// [`UpdateError::ResponseTooLarge`] when either exceeds its size limit;
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

/// Downloads the artifact of `offer` into the cache directory, verifies its
/// digest and its signature, and returns the path of the file.
///
/// The bytes are held in memory until both checks pass and only then written,
/// so a failed download or a failed check leaves no file. A file left behind
/// by a write that failed part-way is removed.
///
/// # Errors
///
/// Returns [`UpdateError::CacheIo`] when the cache directory cannot be
/// created or made private, or the file cannot be written;
/// [`UpdateError::ArtifactUrl`] when the artifact URL, or a redirect from it,
/// is off the allow-list; [`UpdateError::Network`] when the download fails or
/// the server answers 204; [`UpdateError::ResponseTooLarge`] when the
/// artifact exceeds [`MAX_ARTIFACT_BYTES`]; and
/// [`UpdateError::ArtifactIntegrity`] when the digest or the signature does
/// not match.
pub(crate) fn download_and_verify(config: &ClientConfig, offer: &VerifiedOffer) -> Result<PathBuf> {
    prepare_cache_dir(&config.cache_dir).map_err(UpdateError::CacheIo)?;
    purge_cache(&config.cache_dir);

    // The digest makes the name unique to these exact bytes; the URL's file
    // name follows it for the sake of its extension.
    let destination = config.cache_dir.join(format!(
        "{}-{}",
        to_hex(&offer.sha256),
        artifact_file_name(&offer.artifact_url)
    ));

    match download_and_verify_into(config, offer, &destination) {
        Ok(()) => Ok(destination),
        Err(error) => {
            delete_artifact(&destination);
            Err(error)
        }
    }
}

/// Removes the files earlier installs left in `cache_dir`.
///
/// Best effort: a file that cannot be removed, such as an installer that is
/// still running on Windows, stays until a later download clears it. Every
/// failure is logged and none is returned. A leftover of another release
/// does not stand in the way of the download that follows, whose file name
/// starts with its own digest. A leftover of the same release that cannot be
/// removed does: the write refuses the existing path and the install fails
/// with [`UpdateError::CacheIo`].
///
/// Symbolic links are removed too, never followed: `file_type` describes the
/// entry itself. Directories are left alone; this crate creates none here.
pub(crate) fn purge_cache(cache_dir: &Path) {
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
                if !is_directory {
                    delete_artifact(&entry.path());
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

/// Logs `error` under `context`, with its cause when it has one.
///
/// The message of an [`UpdateError`] leaves the cause out, so that it does
/// not repeat it when a caller prints the chain. A failure that is only
/// logged has no such caller, and without the cause a cache failure would
/// not say which operation the system refused.
fn log_failure(context: &str, error: &UpdateError) {
    match std::error::Error::source(error) {
        Some(cause) => log::warn!("{context}: {error}: {cause}"),
        None => log::warn!("{context}: {error}"),
    }
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

/// Downloads the artifact of `offer`, verifies it, and writes it to
/// `destination`.
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

    if sha256(&bytes) != offer.sha256 {
        return Err(UpdateError::ArtifactIntegrity);
    }
    verify_minisign(&config.public_key, &bytes, &offer.artifact_signature)
        .map_err(|_| UpdateError::ArtifactIntegrity)?;

    write_new_private_file(destination, &bytes).map_err(UpdateError::CacheIo)
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
/// off the allow-list; [`UpdateError::ResponseTooLarge`] when the body
/// exceeds the cap of `resource`; [`UpdateError::ManifestSignature`] when
/// `resource` is the feed signature and the server answers 404; and
/// [`UpdateError::Network`] for every other failure.
fn fetch(config: &ClientConfig, url: &Url, resource: Resource) -> Result<Fetched> {
    let mut request_url = url.clone();
    if resource.names_this_copy() {
        append_identity_query(&mut request_url, config);
    }

    fetch_following_redirects(config, request_url, resource).map_err(|failure| match failure {
        FetchFailure::Denied => UpdateError::ArtifactUrl,
        FetchFailure::TooLarge => UpdateError::ResponseTooLarge,
        // A feed published without its signature is a feed that cannot be
        // trusted, which is a different finding from a server in trouble.
        FetchFailure::Status(404) if matches!(resource, Resource::ManifestSignature) => {
            UpdateError::ManifestSignature
        }
        FetchFailure::Status(_) | FetchFailure::Network => UpdateError::Network,
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
    use super::{check_feed_as_client, write_new_private_file};
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
