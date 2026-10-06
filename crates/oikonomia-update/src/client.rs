//! Feed fetch, signed-manifest check, and artifact download. No Tauri types.

use crate::error::{Result, UpdateError};
use crate::hosts::HostPolicy;
use crate::notes::sanitize_notes;
use crate::status::UpdateStatus;
use crate::verify::{parse_public_key, parse_sha256_hex, to_hex, verify_minisign};
use minisign_verify::PublicKey;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

/// `latest.json` of the newest published release in the source repository.
/// GitHub resolves `releases/latest` to published, non-prerelease releases
/// only, so a draft awaiting promotion is never offered.
/// Trust is the baked minisign key, not GitHub.
pub const UPDATE_FEED_URL: &str =
    "https://github.com/ourovoros-io/oikonomia/releases/latest/download/latest.json";

/// Bounds the feed body held in memory.
pub(crate) const MAX_MANIFEST_BYTES: usize = 1_048_576;

/// Bounds the detached feed signature held in memory.
pub(crate) const MAX_SIGNATURE_BYTES: usize = 16_384;

/// Bounds the artifact held in memory while it is verified.
pub(crate) const MAX_ARTIFACT_BYTES: usize = 200 * 1024 * 1024;

/// Bounds how many redirects one fetch follows before it is given up.
pub(crate) const MAX_REDIRECTS: u8 = 5;

/// Bounds one whole feed or signature request: connecting, then everything up
/// to the last byte of the body. A redirect starts a new request with a new
/// deadline. A slow DNS lookup may exceed it: ureq cannot interrupt one
/// (`AgentBuilder::timeout` docs).
const METADATA_DEADLINE: Duration = Duration::from_secs(20);

/// Bounds opening the connection for one artifact request. The DNS lookup
/// before it is not bounded, for the reason given above.
const ARTIFACT_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Bounds each socket read of an artifact response. A download that stalls
/// for this long fails; one that keeps delivering bytes has no time limit,
/// because an artifact of up to [`MAX_ARTIFACT_BYTES`] can take minutes.
const ARTIFACT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// `{os}-{arch}` used by Tauri static manifests (`linux-x86_64`, `darwin-aarch64`).
#[must_use]
pub fn current_updater_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

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

/// Result of [`crate::UpdateMachine::install`] from a legal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Download, verification, or the installer failed; nothing was replaced.
    Failed,
    /// The installer ran; the caller restarts or exits as the handoff says.
    Installed(InstallHandoff),
}

/// Inputs for one check or install. The webview cannot supply a URL or pubkey.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    feed_url: Url,
    public_key: PublicKey,
    current_version: Version,
    platform: String,
    metadata_deadline: Duration,
    cache_dir: PathBuf,
    host_policy: HostPolicy,
    install_route: InstallRoute,
}

impl ClientConfig {
    /// Production constructor: baked feed URL, production host policy, this binary's platform.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError::MissingPublicKey`] when `public_key` is empty or invalid,
    /// [`UpdateError::InvalidFeedUrl`] when the feed constant does not parse,
    /// [`UpdateError::ArtifactUrl`] when the feed constant is not on the production
    /// allow-list, or [`UpdateError::ManifestParse`] when `current_version` is not
    /// `SemVer`.
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
        let config = Self::new(
            feed_url,
            public_key,
            current_version,
            current_updater_platform(),
            cache_dir,
            host_policy,
            METADATA_DEADLINE,
        )?;
        Ok(Self {
            install_route,
            ..config
        })
    }

    fn new(
        feed_url: Url,
        public_key: &str,
        current_version: &str,
        platform: String,
        cache_dir: PathBuf,
        host_policy: HostPolicy,
        metadata_deadline: Duration,
    ) -> Result<Self> {
        let public_key = parse_public_key(public_key)?;
        let current_version = current_version.trim().trim_start_matches('v');
        let current_version =
            Version::parse(current_version).map_err(|_| UpdateError::ManifestParse)?;
        Ok(Self {
            feed_url,
            public_key,
            current_version,
            platform,
            metadata_deadline,
            cache_dir,
            host_policy,
            install_route: InstallRoute::InApp,
        })
    }

    /// Directory where artifacts are written. Callers pass a directory under
    /// the user's own cache location: never the vault data dir, and never a
    /// directory other accounts can write to.
    #[must_use]
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

#[cfg(test)]
impl ClientConfig {
    pub(crate) fn for_test(
        feed_url: Url,
        public_key: &str,
        current_version: &str,
        platform: impl Into<String>,
        cache_dir: PathBuf,
        host_policy: HostPolicy,
        metadata_deadline: Duration,
    ) -> Result<Self> {
        Self::new(
            feed_url,
            public_key,
            current_version,
            platform.into(),
            cache_dir,
            host_policy,
            metadata_deadline,
        )
    }

    pub(crate) fn with_install_route(self, install_route: InstallRoute) -> Self {
        Self {
            install_route,
            ..self
        }
    }
}

/// Artifact metadata taken only after the detached manifest signature verifies.
#[derive(Debug, Clone)]
pub struct VerifiedOffer {
    /// Remote version string (without a required leading `v`).
    pub version: String,
    /// Sanitized notes.
    pub notes: String,
    artifact_url: Url,
    artifact_signature: String,
    sha256: [u8; 32],
    install_route: InstallRoute,
}

impl VerifiedOffer {
    /// The status this offer puts the machine in: installable in-app, or
    /// only reported when the package manager owns the files.
    #[must_use]
    pub fn status(&self) -> UpdateStatus {
        let version = self.version.clone();
        let notes = self.notes.clone();
        match self.install_route {
            InstallRoute::InApp => UpdateStatus::Available { version, notes },
            InstallRoute::PackageManager => UpdateStatus::AvailableManually { version, notes },
        }
    }

    /// How this copy of the app may take the offer.
    #[must_use]
    pub fn install_route(&self) -> InstallRoute {
        self.install_route
    }
}

/// Outcome of [`perform_check`]. Never a URL.
#[derive(Debug, Clone)]
pub enum CheckOutcome {
    /// Same version, older version, or HTTP 204.
    UpToDate,
    /// Newer signed manifest with an allow-listed artifact URL.
    Available(VerifiedOffer),
    /// Network, signature, parse, or allow-list failure.
    Failed,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    platforms: HashMap<String, RawPlatform>,
}

#[derive(Debug, Deserialize)]
struct RawPlatform {
    url: String,
    signature: String,
    sha256: String,
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
    fn max_bytes(self) -> usize {
        match self {
            Self::Manifest => MAX_MANIFEST_BYTES,
            Self::ManifestSignature => MAX_SIGNATURE_BYTES,
            Self::Artifact => MAX_ARTIFACT_BYTES,
        }
    }

    /// Only the feed endpoints are told the version and platform, which is
    /// what lets a feed server answer 204 to a copy that is current. The
    /// artifact URL comes from the manifest and is fetched exactly as signed.
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

enum FetchFail {
    Network,
    Denied,
    TooLarge,
}

/// Fetches the signed manifest. Does **not** download the artifact.
#[must_use]
pub fn perform_check(config: &ClientConfig) -> CheckOutcome {
    match perform_check_inner(config) {
        Ok(outcome) => outcome,
        Err(err) => {
            log::warn!("update check failed: {err}");
            CheckOutcome::Failed
        }
    }
}

/// Runs the check and keeps the cause of a failure, which [`perform_check`]
/// logs and reduces to [`CheckOutcome::Failed`].
pub(crate) fn perform_check_inner(config: &ClientConfig) -> Result<CheckOutcome> {
    let Fetched::Body(body) = fetch_bytes(config, &config.feed_url, Resource::Manifest)? else {
        return Ok(CheckOutcome::UpToDate);
    };

    // Derived from the configured URL, not from the URL that was requested:
    // `fetch_bytes` appends the identity query itself, once per request.
    let signature_url = signature_url_for(&config.feed_url);
    let Fetched::Body(signature_bytes) =
        fetch_bytes(config, &signature_url, Resource::ManifestSignature)?
    else {
        return Err(UpdateError::ManifestSignature);
    };
    let signature =
        std::str::from_utf8(&signature_bytes).map_err(|_| UpdateError::ManifestSignature)?;
    verify_minisign(&config.public_key, &body, signature)?;

    let manifest: RawManifest =
        serde_json::from_slice(&body).map_err(|_| UpdateError::ManifestParse)?;

    // The version is compared before the platform entry is read: a copy that
    // is already current has no use for an artifact, so a feed that lists
    // none for its platform is not a failure for it.
    let remote = manifest.version.trim().trim_start_matches('v');
    let remote = Version::parse(remote).map_err(|_| UpdateError::ManifestParse)?;
    if remote <= config.current_version {
        return Ok(CheckOutcome::UpToDate);
    }

    let offer = offer_from_manifest(config, &manifest)?;
    Ok(CheckOutcome::Available(offer))
}

fn offer_from_manifest(config: &ClientConfig, manifest: &RawManifest) -> Result<VerifiedOffer> {
    let platform = manifest
        .platforms
        .get(&config.platform)
        .ok_or(UpdateError::MissingPlatform)?;

    let artifact_url = Url::parse(&platform.url).map_err(|_| UpdateError::ArtifactUrl)?;
    if !config.host_policy.is_allowed_artifact_url(&artifact_url) {
        return Err(UpdateError::ArtifactUrl);
    }
    if platform.signature.trim().is_empty() {
        return Err(UpdateError::ManifestSignature);
    }
    let sha256 = parse_sha256_hex(&platform.sha256)?;
    let notes = sanitize_notes(manifest.notes.as_deref().unwrap_or(""));
    Ok(VerifiedOffer {
        version: manifest.version.clone(),
        notes,
        artifact_url,
        artifact_signature: platform.signature.clone(),
        sha256,
        install_route: config.install_route,
    })
}

/// Downloads the artifact outside the vault, verifies hash and minisign, then returns the path.
///
/// The bytes are held in memory until both checks pass and only then written,
/// so a failed download or a failed check leaves no file. A file left behind
/// by a write that failed part-way is removed.
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactUrl`] when the artifact URL, or a redirect
/// from it, is off the allow-list; [`UpdateError::ArtifactIntegrity`] when the
/// hash or the signature does not match; [`UpdateError::ArtifactTooLarge`]
/// when the artifact exceeds its size cap; and
/// [`UpdateError::Network`] when the download fails, or when the cache
/// directory or the file cannot be written.
pub fn download_and_verify(config: &ClientConfig, offer: &VerifiedOffer) -> Result<PathBuf> {
    if let Err(err) = prepare_cache_dir(&config.cache_dir) {
        log::warn!("updater cache create failed: {err}");
        return Err(UpdateError::Network);
    }
    purge_cache(&config.cache_dir);
    let dest = config.cache_dir.join(format!(
        "{}-{}",
        to_hex(&offer.sha256),
        artifact_file_name(&offer.artifact_url)
    ));
    match download_and_verify_inner(config, offer, &dest) {
        Ok(()) => Ok(dest),
        Err(err) => {
            delete_artifact(&dest);
            Err(err)
        }
    }
}

fn download_and_verify_inner(
    config: &ClientConfig,
    offer: &VerifiedOffer,
    dest: &Path,
) -> Result<()> {
    if !config
        .host_policy
        .is_allowed_artifact_url(&offer.artifact_url)
    {
        return Err(UpdateError::ArtifactUrl);
    }
    let Fetched::Body(bytes) = fetch_bytes(config, &offer.artifact_url, Resource::Artifact)? else {
        return Err(UpdateError::Network);
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    if digest.as_slice() != offer.sha256 {
        return Err(UpdateError::ArtifactIntegrity);
    }
    verify_minisign(&config.public_key, &bytes, &offer.artifact_signature)
        .map_err(|_| UpdateError::ArtifactIntegrity)?;
    write_new_private_file(dest, &bytes).map_err(|err| {
        log::warn!("updater artifact write failed: {err}");
        UpdateError::Network
    })?;
    Ok(())
}

/// Creates the cache directory for this user only.
///
/// The verified artifact is read back from here by path and then run, so
/// nobody else may be able to swap it in between. On Unix the directory is
/// forced to mode `0700`; that fails, and the install with it, when the
/// directory belongs to another account. The caller passes a directory under
/// the user's own cache location, never a shared temporary directory.
fn prepare_cache_dir(cache_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_dir)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(cache_dir, std::fs::Permissions::from_mode(0o700))?;
    }

    Ok(())
}

/// Writes `bytes` to a file that must not exist yet. `create_new` refuses an
/// existing path, a symbolic link included, so the write cannot be
/// redirected to a file outside the cache.
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

/// Local file name for a downloaded artifact: the last segment of its URL,
/// reduced to ASCII letters, digits, `.`, `-` and `_`.
///
/// The extension has to survive the download. The installers choose their
/// action from it, and Windows will not start a program whose name has no
/// extension.
fn artifact_file_name(url: &Url) -> String {
    const MAX_NAME_CHARS: usize = 96;

    let segment = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or("");

    let mut name = String::new();
    for ch in segment.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            name.push(ch);
        } else {
            name.push('_');
        }
    }

    // Keep the tail: that is where the extension lives.
    let excess = name.len().saturating_sub(MAX_NAME_CHARS);
    let name = name.split_off(excess);
    let name = name.trim_start_matches('.');

    if name.is_empty() {
        "artifact".to_owned()
    } else {
        name.to_owned()
    }
}

/// Removes files left by earlier installs. Best effort: an installer that is
/// still running keeps its file, which the next download clears.
///
/// Symbolic links are removed too, never followed: `file_type` describes the
/// entry itself.
fn purge_cache(cache_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if !is_directory {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Removes the artifact at `path`. Used after a failed download or install,
/// and once an installer has replaced the app.
///
/// A file that is already gone is not a failure. Any other failure is logged
/// and not returned: this is cleanup, and it must not replace the outcome of
/// the step it follows.
pub fn delete_artifact(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            log::warn!(
                "updater artifact removal failed for {}: {err}",
                path.display()
            );
        }
    }
}

fn signature_url_for(feed: &Url) -> Url {
    let mut signature = feed.clone();
    let path = format!("{}.sig", feed.path());
    signature.set_path(&path);
    signature
}

fn fetch_bytes(config: &ClientConfig, url: &Url, resource: Resource) -> Result<Fetched> {
    let mut request_url = url.clone();
    if resource.names_this_copy() {
        attach_version_os_arch(&mut request_url, config);
    }

    fetch_once(config, &request_url, resource).map_err(|fail| match (fail, resource) {
        (FetchFail::Denied, _) => UpdateError::ArtifactUrl,
        // Retrying cannot help, so the user must not be told to check the
        // connection.
        (FetchFail::TooLarge, Resource::Artifact) => UpdateError::ArtifactTooLarge,
        (FetchFail::TooLarge, Resource::Manifest | Resource::ManifestSignature)
        | (FetchFail::Network, _) => UpdateError::Network,
    })
}

fn fetch_once(
    config: &ClientConfig,
    start: &Url,
    resource: Resource,
) -> std::result::Result<Fetched, FetchFail> {
    let agent = agent_for(config, resource);

    let mut url = start.clone();
    let mut redirects_followed = 0_u8;
    loop {
        if !config.host_policy.is_allowed_fetch_url(&url) {
            return Err(FetchFail::Denied);
        }

        // With `redirects(0)` ureq 2 hands a 3xx back as `Ok` (`connect` in
        // its `unit.rs`) and reports only 4xx and 5xx as `Error::Status`, so
        // redirects are followed here, one policy check per hop.
        let response = agent.get(url.as_str()).call().map_err(|err| {
            log::warn!("update fetch failed: {err}");
            FetchFail::Network
        })?;

        match response.status() {
            200 => {
                let bytes = read_capped(response, resource.max_bytes())?;
                return Ok(Fetched::Body(bytes));
            }
            204 => return Ok(Fetched::NoContent),
            status if is_redirect(status) => {
                if redirects_followed == MAX_REDIRECTS {
                    return Err(FetchFail::Network);
                }
                redirects_followed += 1;

                let Some(location) = response.header("Location") else {
                    return Err(FetchFail::Network);
                };
                url = resolve_redirect(&url, location)?;
            }
            _ => return Err(FetchFail::Network),
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

fn is_redirect(code: u16) -> bool {
    code == 301 || code == 302 || code == 303 || code == 307 || code == 308
}

fn resolve_redirect(current: &Url, location: &str) -> std::result::Result<Url, FetchFail> {
    current.join(location).map_err(|_| FetchFail::Network)
}

fn attach_version_os_arch(url: &mut Url, config: &ClientConfig) {
    let (os, arch) = split_platform(&config.platform);
    let mut pairs = url.query_pairs_mut();
    pairs.append_pair("version", &config.current_version.to_string());
    pairs.append_pair("os", os);
    pairs.append_pair("arch", arch);
}

fn split_platform(platform: &str) -> (&str, &str) {
    match platform.split_once('-') {
        Some((os, arch)) => (os, arch),
        None => (platform, ""),
    }
}

fn read_capped(
    response: ureq::Response,
    max_bytes: usize,
) -> std::result::Result<Vec<u8>, FetchFail> {
    let mut reader = response.into_reader();
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return Err(FetchFail::Network),
        };
        if buf.len().saturating_add(read) > max_bytes {
            return Err(FetchFail::TooLarge);
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    Ok(buf)
}

/// Platform installer invoked only after hash and signature succeed.
pub trait ArtifactInstaller {
    /// Executes the already-verified artifact.
    ///
    /// # Errors
    ///
    /// Returns an [`UpdateError`] when the platform installer cannot run.
    fn install(&self, artifact: &Path) -> Result<InstallHandoff>;
}
