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

/// GitHub Releases CDN for `latest.json`. Trust is the baked minisign key, not GitHub.
pub const UPDATE_FEED_URL: &str =
    "https://github.com/GeorgiosDelkos/oikonomia/releases/latest/download/latest.json";

const MAX_MANIFEST_BYTES: usize = 1_048_576;
const MAX_SIGNATURE_BYTES: usize = 16_384;
const MAX_ARTIFACT_BYTES: usize = 200 * 1024 * 1024;
const MAX_REDIRECTS: u8 = 5;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// OS temp subdirectory used as the updater cache. Never the vault data dir.
#[must_use]
pub fn default_updater_cache_dir() -> PathBuf {
    std::env::temp_dir().join("oikonomia-updater")
}

/// `{os}-{arch}` used by Tauri static manifests (`linux-x86_64`, `darwin-aarch64`).
#[must_use]
pub fn current_updater_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

/// Inputs for one check or install. The webview cannot supply a URL or pubkey.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    feed_url: Url,
    public_key: PublicKey,
    current_version: Version,
    platform: String,
    timeout: Duration,
    cache_dir: PathBuf,
    host_policy: HostPolicy,
}

impl ClientConfig {
    /// Production constructor: baked feed URL, production host policy, this binary's platform.
    ///
    /// # Errors
    ///
    /// Returns [`UpdateError::MissingPublicKey`] when `public_key` is empty or invalid,
    /// [`UpdateError::InvalidFeedUrl`] when the feed constant does not parse, or
    /// [`UpdateError::ManifestParse`] when `current_version` is not SemVer.
    pub fn production(
        public_key: &str,
        current_version: &str,
        cache_dir: PathBuf,
    ) -> Result<Self> {
        let feed_url = Url::parse(UPDATE_FEED_URL).map_err(|_| UpdateError::InvalidFeedUrl)?;
        let host_policy = HostPolicy::production();
        if !host_policy.is_allowed_fetch_url(&feed_url) {
            return Err(UpdateError::ArtifactUrl);
        }
        Self::new(
            feed_url,
            public_key,
            current_version,
            current_updater_platform(),
            cache_dir,
            host_policy,
            DEFAULT_TIMEOUT,
        )
    }

    fn new(
        feed_url: Url,
        public_key: &str,
        current_version: &str,
        platform: String,
        cache_dir: PathBuf,
        host_policy: HostPolicy,
        timeout: Duration,
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
            timeout,
            cache_dir,
            host_policy,
        })
    }

    /// Directory where artifacts are written. Callers must not pass the vault data dir.
    #[must_use]
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

#[cfg(test)]
impl ClientConfig {
    #[allow(clippy::too_many_arguments, reason = "test fixture constructor")]
    pub(crate) fn for_test(
        feed_url: Url,
        public_key: &str,
        current_version: &str,
        platform: impl Into<String>,
        cache_dir: PathBuf,
        host_policy: HostPolicy,
        timeout: Duration,
    ) -> Result<Self> {
        Self::new(
            feed_url,
            public_key,
            current_version,
            platform.into(),
            cache_dir,
            host_policy,
            timeout,
        )
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
}

impl VerifiedOffer {
    /// Version and notes for [`UpdateStatus::Available`].
    #[must_use]
    pub fn status(&self) -> UpdateStatus {
        UpdateStatus::Available {
            version: self.version.clone(),
            notes: self.notes.clone(),
        }
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
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPlatform {
    url: String,
    signature: String,
    sha256: String,
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

fn perform_check_inner(config: &ClientConfig) -> Result<CheckOutcome> {
    let (final_url, body, status) =
        fetch_bytes(config, &config.feed_url, MAX_MANIFEST_BYTES, true)?;
    if status == 204 {
        return Ok(CheckOutcome::UpToDate);
    }
    if status != 200 {
        return Err(UpdateError::Network);
    }

    let signature_url = signature_url_for(&final_url)?;
    let (_sig_url, signature_bytes, sig_status) =
        fetch_bytes(config, &signature_url, MAX_SIGNATURE_BYTES, true)?;
    if sig_status != 200 {
        return Err(UpdateError::ManifestSignature);
    }
    let signature = std::str::from_utf8(&signature_bytes).map_err(|_| UpdateError::ManifestSignature)?;
    verify_minisign(&config.public_key, &body, signature)?;

    let manifest: RawManifest =
        serde_json::from_slice(&body).map_err(|_| UpdateError::ManifestParse)?;
    let offer = offer_from_manifest(config, &manifest)?;
    let remote = offer.version.trim().trim_start_matches('v');
    let remote = Version::parse(remote).map_err(|_| UpdateError::ManifestParse)?;
    if remote <= config.current_version {
        return Ok(CheckOutcome::UpToDate);
    }
    Ok(CheckOutcome::Available(offer))
}

fn offer_from_manifest(config: &ClientConfig, manifest: &RawManifest) -> Result<VerifiedOffer> {
    let (url_text, signature, sha256_hex) = if let Some(platform) = manifest.platforms.get(&config.platform)
    {
        (
            platform.url.as_str(),
            platform.signature.as_str(),
            platform.sha256.as_str(),
        )
    } else {
        let url = manifest.url.as_deref().ok_or(UpdateError::ManifestParse)?;
        let signature = manifest
            .signature
            .as_deref()
            .ok_or(UpdateError::ManifestParse)?;
        let sha256 = manifest
            .sha256
            .as_deref()
            .ok_or(UpdateError::ArtifactIntegrity)?;
        (url, signature, sha256)
    };

    let artifact_url = Url::parse(url_text).map_err(|_| UpdateError::ArtifactUrl)?;
    if !config.host_policy.is_allowed_artifact_url(&artifact_url) {
        return Err(UpdateError::ArtifactUrl);
    }
    if signature.trim().is_empty() {
        return Err(UpdateError::ManifestSignature);
    }
    let sha256 = parse_sha256_hex(sha256_hex)?;
    let notes = sanitize_notes(manifest.notes.as_deref().unwrap_or(""));
    Ok(VerifiedOffer {
        version: manifest.version.clone(),
        notes,
        artifact_url,
        artifact_signature: signature.to_owned(),
        sha256,
    })
}

/// Downloads the artifact outside the vault, verifies hash and minisign, then returns the path.
///
/// On any failure the partial file is deleted.
///
/// # Errors
///
/// Returns [`UpdateError::ArtifactIntegrity`] or [`UpdateError::Network`] / [`UpdateError::ArtifactUrl`].
pub fn download_and_verify(config: &ClientConfig, offer: &VerifiedOffer) -> Result<PathBuf> {
    if let Err(err) = std::fs::create_dir_all(&config.cache_dir) {
        log::warn!("updater cache create failed: {err}");
        return Err(UpdateError::Network);
    }
    let dest = config
        .cache_dir
        .join(format!("artifact-{}", to_hex(&offer.sha256)));
    match download_and_verify_inner(config, offer, &dest) {
        Ok(()) => Ok(dest),
        Err(err) => {
            let _ = std::fs::remove_file(&dest);
            Err(err)
        }
    }
}

fn download_and_verify_inner(
    config: &ClientConfig,
    offer: &VerifiedOffer,
    dest: &Path,
) -> Result<()> {
    if !config.host_policy.is_allowed_artifact_url(&offer.artifact_url) {
        return Err(UpdateError::ArtifactUrl);
    }
    let (_url, bytes, status) =
        fetch_bytes(config, &offer.artifact_url, MAX_ARTIFACT_BYTES, false)?;
    if status != 200 {
        return Err(UpdateError::Network);
    }
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    if digest.as_slice() != offer.sha256 {
        return Err(UpdateError::ArtifactIntegrity);
    }
    verify_minisign(&config.public_key, &bytes, &offer.artifact_signature)
        .map_err(|_| UpdateError::ArtifactIntegrity)?;
    std::fs::write(dest, &bytes).map_err(|err| {
        log::warn!("updater artifact write failed: {err}");
        UpdateError::Network
    })?;
    Ok(())
}

/// Removes `path` if it exists. Used after a failed install or after exec.
pub fn delete_artifact(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

fn signature_url_for(feed: &Url) -> Result<Url> {
    let mut signature = feed.clone();
    let path = format!("{}.sig", feed.path());
    signature.set_path(&path);
    Ok(signature)
}

fn fetch_bytes(
    config: &ClientConfig,
    url: &Url,
    max_bytes: usize,
    attach_identity: bool,
) -> std::result::Result<(Url, Vec<u8>, u16), UpdateError> {
    let mut url = url.clone();
    if attach_identity {
        attach_version_os_arch(&mut url, config);
    }
    if !config.host_policy.is_allowed_fetch_url(&url) {
        return Err(UpdateError::ArtifactUrl);
    }
    match fetch_once(config, &url, max_bytes) {
        Ok((bytes, status)) => Ok((url, bytes, status)),
        Err(FetchFail::Denied) => Err(UpdateError::ArtifactUrl),
        Err(FetchFail::TooLarge | FetchFail::Network) => Err(UpdateError::Network),
    }
}

fn fetch_once(
    config: &ClientConfig,
    start: &Url,
    max_bytes: usize,
) -> std::result::Result<(Vec<u8>, u16), FetchFail> {
    let agent = ureq::AgentBuilder::new()
        .timeout(config.timeout)
        .redirects(0)
        .user_agent(&format!("Oikonomia/{}", config.current_version))
        .build();

    let mut url = start.clone();
    let mut hop = 0;
    loop {
        if !config.host_policy.is_allowed_fetch_url(&url) {
            return Err(FetchFail::Denied);
        }
        match agent.get(url.as_str()).call() {
            Ok(response) => {
                let status = response.status();
                if status == 204 {
                    return Ok((Vec::new(), 204));
                }
                if status != 200 {
                    return Err(FetchFail::Network);
                }
                let bytes = read_capped(response, max_bytes)?;
                return Ok((bytes, status));
            }
            Err(ureq::Error::Status(code, response)) if is_redirect(code) => {
                hop += 1;
                if hop > MAX_REDIRECTS {
                    return Err(FetchFail::Network);
                }
                let Some(location) = response.header("Location") else {
                    return Err(FetchFail::Network);
                };
                url = resolve_redirect(&url, location)?;
            }
            Err(_) => return Err(FetchFail::Network),
        }
    }
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

fn read_capped(response: ureq::Response, max_bytes: usize) -> std::result::Result<Vec<u8>, FetchFail> {
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
    fn install(&self, artifact: &Path) -> Result<()>;
}
