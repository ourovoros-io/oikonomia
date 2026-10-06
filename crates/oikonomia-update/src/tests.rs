//! Local httptest fixtures. Never contacts production GitHub.

#![expect(clippy::expect_used, reason = "tests fail loudly by design")]
#![expect(clippy::panic, reason = "tests fail loudly by design")]

use crate::UPDATE_FEED_URL;
use crate::client::{
    ArtifactInstaller, CheckOutcome, ClientConfig, InstallHandoff, InstallOutcome, InstallRoute,
    MAX_ARTIFACT_BYTES, MAX_MANIFEST_BYTES, MAX_REDIRECTS, MAX_SIGNATURE_BYTES, VerifiedOffer,
    delete_artifact, download_and_verify, perform_check, perform_check_inner,
};
use crate::error::UpdateError;
use crate::feed::{FeedArtifact, assemble_manifest};
use crate::hosts::HostPolicy;
use crate::machine::UpdateMachine;
use crate::notes::sanitize_notes;
use crate::status::UpdateStatus;
use crate::verify::{parse_public_key, to_hex};
use httptest::responders::status_code;
use httptest::{Expectation, Server, matchers::request};
use minisign::{KeyPair, SecretKey};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use url::Url;

struct SpyInstaller {
    calls: Arc<AtomicUsize>,
    last_path: Mutex<Option<PathBuf>>,
    fail: bool,
    handoff: InstallHandoff,
}

impl ArtifactInstaller for SpyInstaller {
    fn install(&self, artifact: &Path) -> crate::Result<InstallHandoff> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut last = self.last_path.lock().expect("spy path");
        *last = Some(artifact.to_path_buf());
        if self.fail {
            return Err(UpdateError::ArtifactIntegrity);
        }
        Ok(self.handoff)
    }
}

fn spy(fail: bool) -> (SpyInstaller, Arc<AtomicUsize>) {
    spy_with_handoff(fail, InstallHandoff::Replaced)
}

fn spy_with_handoff(fail: bool, handoff: InstallHandoff) -> (SpyInstaller, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        SpyInstaller {
            calls: Arc::clone(&calls),
            last_path: Mutex::new(None),
            fail,
            handoff,
        },
        calls,
    )
}

/// Serves a signed manifest offering `payload` at `artifact_path` as 0.2.0
/// and returns a config for an app at 0.1.0.
fn serve_newer_release(
    server: &Server,
    public_key: &str,
    secret_key: &SecretKey,
    artifact_path: &'static str,
    payload: &'static [u8],
    cache: &Path,
) -> ClientConfig {
    let artifact = server_url(server, artifact_path);
    let body = static_manifest(
        "0.2.0",
        "ok",
        artifact.as_str(),
        &sign(secret_key, payload),
        &sha256_hex(payload),
    );
    let sig = sign(secret_key, body.as_bytes());
    serve_signed_manifest(server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", artifact_path))
            .times(0..)
            .respond_with(status_code(200).body(payload)),
    );
    config(
        server,
        "/latest.json",
        public_key,
        "0.1.0",
        cache,
        Duration::from_secs(2),
    )
}

fn test_keys() -> (String, SecretKey) {
    let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().expect("keypair");
    let boxed = pk.to_box().expect("public box");
    (boxed.to_string(), sk)
}

fn sign(sk: &SecretKey, data: &[u8]) -> String {
    let signature = minisign::sign(None, sk, Cursor::new(data), None, None).expect("sign");
    signature.into_string()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    to_hex(&hasher.finalize())
}

/// Returns a fresh cache directory. The caller holds the handle for the whole
/// test: dropping it removes the directory.
fn cache_dir() -> TempDir {
    tempfile::tempdir().expect("temporary directory")
}

fn server_url(server: &Server, path: &str) -> Url {
    Url::parse(&server.url_str(path)).expect("server url")
}

fn policy_for(server: &Server) -> HostPolicy {
    let url = server_url(server, "/");
    let host = url.host_str().expect("host").to_owned();
    HostPolicy::test_http_hosts([host])
}

fn config(
    server: &Server,
    feed_path: &str,
    public_key: &str,
    current_version: &str,
    cache: &Path,
    timeout: Duration,
) -> ClientConfig {
    let feed_url = server_url(server, feed_path);
    ClientConfig::for_test(
        feed_url,
        public_key,
        current_version,
        "linux-x86_64",
        cache.to_path_buf(),
        policy_for(server),
        timeout,
    )
    .expect("config")
}

fn static_manifest(
    version: &str,
    notes: &str,
    artifact_url: &str,
    signature: &str,
    sha256: &str,
) -> String {
    format!(
        r#"{{
  "version": "{version}",
  "notes": {notes_json},
  "platforms": {{
    "linux-x86_64": {{
      "url": "{artifact_url}",
      "signature": {sig_json},
      "sha256": "{sha256}"
    }}
  }}
}}"#,
        notes_json = serde_json::to_string(notes).expect("notes json"),
        sig_json = serde_json::to_string(signature).expect("sig json"),
    )
}

fn serve_signed_manifest(server: &Server, body: &str, sig: &str) {
    serve_signed_manifest_times(server, body, sig, 1..);
}

fn serve_signed_manifest_times(
    server: &Server,
    body: &str,
    sig: &str,
    times: std::ops::RangeFrom<usize>,
) {
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .times(times.clone())
            .respond_with(status_code(200).body(body.to_owned())),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .times(times)
            .respond_with(status_code(200).body(sig.to_owned())),
    );
}

fn serve_signed_manifest_once(server: &Server, body: &str, sig: &str) {
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .times(1)
            .respond_with(status_code(200).body(body.to_owned())),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .times(1)
            .respond_with(status_code(200).body(sig.to_owned())),
    );
}

fn leftover_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        found.push(entry.path());
    }
    found
}

#[test]
fn empty_public_key_is_rejected() {
    let err = parse_public_key("").expect_err("empty");
    assert_eq!(err.code(), "update_missing_public_key");
    let err = parse_public_key("   ").expect_err("whitespace");
    assert_eq!(err.code(), "update_missing_public_key");
}

#[test]
fn production_feed_url_is_https_github() {
    let url = Url::parse(UPDATE_FEED_URL).expect("feed");
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("github.com"));
    assert!(url.path().ends_with("/latest.json"));
}

#[test]
fn production_feed_is_the_latest_release_of_this_source_repository() {
    // Releases are cut in the repository the code lives in. Deriving the
    // expectation from the manifest's `repository` means a repo move cannot
    // leave the updater pointing at a feed nobody publishes to.
    let expected = format!(
        "{}/releases/latest/download/latest.json",
        env!("CARGO_PKG_REPOSITORY")
    );

    assert_eq!(UPDATE_FEED_URL, expected);
}

#[test]
fn offline_dns_check_is_failed_no_file_no_exec() {
    let (pk, _sk) = test_keys();
    let cache = cache_dir();
    let feed = Url::parse("http://no-such-host.invalid/latest.json").expect("url");
    let config = ClientConfig::for_test(
        feed,
        &pk,
        "0.1.0",
        "linux-x86_64",
        cache.path().to_path_buf(),
        HostPolicy::test_http_hosts(["no-such-host.invalid"]),
        Duration::from_millis(400),
    )
    .expect("config");
    let (installer, _calls) = spy(false);
    let mut machine = UpdateMachine::new();
    let status = machine.check(&config);
    assert_eq!(status, UpdateStatus::Failed);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
    assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn timeout_check_is_failed_no_file_no_exec() {
    let (pk, _sk) = test_keys();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        let _accepted = listener.accept();
        std::thread::sleep(Duration::from_secs(30));
    });
    let cache = cache_dir();
    let feed = Url::parse(&format!("http://{addr}/latest.json")).expect("url");
    let config = ClientConfig::for_test(
        feed,
        &pk,
        "0.1.0",
        "linux-x86_64",
        cache.path().to_path_buf(),
        HostPolicy::test_http_hosts(["127.0.0.1"]),
        Duration::from_millis(200),
    )
    .expect("config");
    let mut machine = UpdateMachine::new();
    let status = machine.check(&config);
    assert_eq!(status, UpdateStatus::Failed);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn same_version_is_up_to_date_no_download() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let payload = b"artifact-bytes";
    let body = static_manifest(
        "0.1.0",
        "nothing new",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .times(0)
            .respond_with(status_code(500)),
    );

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::UpToDate);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn http_204_is_up_to_date_no_download() {
    let (pk, _sk) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(204)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::UpToDate);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn signed_update_is_available_without_install() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"newer-artifact";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "plain notes",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let (installer, _calls) = spy(false);
    let mut machine = UpdateMachine::new();
    let status = machine.check(&config);
    assert_eq!(
        status,
        UpdateStatus::Available {
            version: "0.2.0".into(),
            notes: "plain notes".into(),
        }
    );
    assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn missing_manifest_sig_is_failed() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"x";
    let body = static_manifest(
        "0.2.0",
        "n",
        server_url(&server, "/Oikonomia.AppImage").as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .times(2)
            .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .times(2)
            .respond_with(status_code(404)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    // The fetch reports every status other than 200 and 204 as a network
    // failure, so a 404 on the signature ends the check before any signature
    // is looked at.
    assert_eq!(check_error_code(&config), "update_network");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn bad_manifest_sig_is_failed() {
    let (pk, sk) = test_keys();
    let (other_pk, other_sk) = test_keys();
    let _ = other_pk;
    let server = Server::run();
    let payload = b"x";
    let body = static_manifest(
        "0.2.0",
        "n",
        server_url(&server, "/Oikonomia.AppImage").as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let wrong = sign(&other_sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &wrong);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn truncated_json_after_valid_sig_is_failed() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let truncated = b"{\"version\":\"0.2.0\"";
    let sig = sign(&sk, truncated);
    serve_signed_manifest(&server, std::str::from_utf8(truncated).expect("utf8"), &sig);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn host_not_allow_listed_is_failed_no_file() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"payload";
    let body = static_manifest(
        "0.2.0",
        "n",
        "https://evil.example/payload",
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn file_url_artifact_is_failed() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"payload";
    let body = static_manifest(
        "0.2.0",
        "n",
        "file:///tmp/evil",
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn artifact_hash_mismatch_leaves_no_file_and_does_not_exec() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "n",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(b"different"),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(status_code(200).body(payload.as_slice())),
    );

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    assert!(matches!(
        machine.check(&config),
        UpdateStatus::Available { .. }
    ));
    let outcome = machine.install(&config, &installer).expect("legal");
    assert_eq!(outcome, InstallOutcome::Failed);
    assert_eq!(machine.status(), UpdateStatus::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn artifact_sig_mismatch_leaves_no_file_and_does_not_exec() {
    let (pk, sk) = test_keys();
    let (_other_pk, other_sk) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "n",
        artifact.as_str(),
        &sign(&other_sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(status_code(200).body(payload.as_slice())),
    );

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    assert!(matches!(
        machine.check(&config),
        UpdateStatus::Available { .. }
    ));
    let outcome = machine.install(&config, &installer).expect("legal");
    assert_eq!(outcome, InstallOutcome::Failed);
    assert_eq!(machine.status(), UpdateStatus::Failed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn install_from_idle_is_hard_error() {
    let mut machine = UpdateMachine::new();
    let (pk, _sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(1),
    );
    let (installer, _calls) = spy(false);
    let err = machine.install(&config, &installer).expect_err("idle");
    assert_eq!(err.code(), "update_install_not_allowed");
    assert_eq!(machine.status(), UpdateStatus::Idle);
}

#[test]
fn install_from_failed_is_hard_error() {
    let (pk, _sk) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(500)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    assert_eq!(machine.check(&config), UpdateStatus::Failed);
    let (installer, _calls) = spy(false);
    let err = machine.install(&config, &installer).expect_err("failed");
    assert_eq!(err.code(), "update_install_not_allowed");
}

#[test]
fn install_from_checking_is_hard_error() {
    let mut machine = UpdateMachine::new();
    machine.begin_check();
    let (pk, _sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(1),
    );
    let (installer, _calls) = spy(false);
    let err = machine.install(&config, &installer).expect_err("checking");
    assert_eq!(err.code(), "update_install_not_allowed");
}

#[test]
fn html_notes_are_plain_text() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"newer";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "Read <a href=\"https://evil.example/nav\">here</a>",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    let UpdateStatus::Available { notes, version } = machine.check(&config) else {
        panic!("expected available");
    };
    assert_eq!(version, "0.2.0");
    assert_eq!(
        notes,
        sanitize_notes("Read <a href=\"https://evil.example/nav\">here</a>")
    );
    assert!(!notes.contains('<') || notes.contains("&lt;"));
    assert!(!notes.contains("href"));
}

#[test]
fn successful_install_calls_exec_once() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"install-me";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "ok",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(payload),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest_once(&server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .times(1)
            .respond_with(status_code(200).body(payload.as_slice())),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();
    machine.check(&config);
    let outcome = machine.install(&config, &installer).expect("legal");
    assert_eq!(outcome, InstallOutcome::Installed(InstallHandoff::Replaced));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let exec_path = installer
        .last_path
        .lock()
        .expect("path")
        .clone()
        .expect("installer received a path");
    assert!(
        exec_path.starts_with(cache.path()),
        "exec path must be the wrapper-verified cache file, got {}",
        exec_path.display()
    );
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn downloaded_artifact_keeps_the_file_extension_of_its_url() {
    // The installers pick their action from the extension, and Windows will
    // not start a program whose file name has none.
    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_newer_release(
        &server,
        &pk,
        &sk,
        "/Oikonomia_0.2.0_x64-setup.exe",
        b"installer",
        cache.path(),
    );
    let (installer, _calls) = spy(false);
    let mut machine = UpdateMachine::new();
    machine.check(&config);
    machine.install(&config, &installer).expect("legal");

    let exec_path = installer
        .last_path
        .lock()
        .expect("path")
        .clone()
        .expect("installer received a path");
    let name = exec_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("file name");
    assert!(
        name.ends_with("-Oikonomia_0.2.0_x64-setup.exe"),
        "unexpected artifact name {name}"
    );
    assert_eq!(exec_path.parent(), Some(cache.path()));
}

#[test]
fn artifact_name_from_a_hostile_url_cannot_leave_the_cache_directory() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    // The last URL segment decodes to `..\..\evil name.exe`.
    let config = serve_newer_release(
        &server,
        &pk,
        &sk,
        "/dir/..%5C..%5Cevil%20name.exe",
        b"installer",
        cache.path(),
    );
    let mut machine = UpdateMachine::new();
    machine.check(&config);
    let offer = machine.require_available().expect("offer").clone();
    let path = download_and_verify(&config, &offer).expect("download");

    assert_eq!(path.parent(), Some(cache.path()));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("file name");
    assert!(
        name.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_'),
        "unexpected artifact name {name}"
    );
    assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("exe"));
}

#[test]
fn a_running_installer_keeps_its_artifact_and_the_next_download_clears_it() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_newer_release(
        &server,
        &pk,
        &sk,
        "/Oikonomia-setup.exe",
        b"installer",
        cache.path(),
    );
    let (installer, _calls) = spy_with_handoff(false, InstallHandoff::InstallerStarted);
    let mut machine = UpdateMachine::new();
    machine.check(&config);

    let outcome = machine.install(&config, &installer).expect("legal");

    assert_eq!(
        outcome,
        InstallOutcome::Installed(InstallHandoff::InstallerStarted)
    );
    assert_eq!(leftover_files(cache.path()).len(), 1);

    let stale = cache.path().join("stale-from-an-earlier-install.exe");
    std::fs::write(&stale, b"old").expect("stale file");
    let offer = machine.require_available().expect("offer").clone();
    let fresh = download_and_verify(&config, &offer).expect("download");
    assert_eq!(leftover_files(cache.path()), vec![fresh]);
}

#[cfg(unix)]
#[test]
fn cache_is_private_and_a_planted_link_cannot_redirect_the_download() {
    use std::os::unix::fs::PermissionsExt;

    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_newer_release(
        &server,
        &pk,
        &sk,
        "/Oikonomia.AppImage",
        b"appimage",
        cache.path(),
    );
    let mut machine = UpdateMachine::new();
    machine.check(&config);
    let offer = machine.require_available().expect("offer").clone();

    // Someone planted a link where the artifact will be written, pointing
    // at a file outside the cache, and loosened the directory.
    let outside_dir = cache_dir();
    let outside = outside_dir.path().join("victim");
    let first = download_and_verify(&config, &offer).expect("first download");
    let planted = first.clone();
    std::fs::remove_file(&first).expect("clear");
    std::os::unix::fs::symlink(&outside, &planted).expect("plant link");
    std::fs::set_permissions(cache.path(), std::fs::Permissions::from_mode(0o777)).expect("loosen");

    let path = download_and_verify(&config, &offer).expect("download");

    assert!(!outside.exists(), "the download followed the planted link");
    let metadata = std::fs::symlink_metadata(&path).expect("metadata");
    assert!(metadata.file_type().is_file());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read(&path).expect("read"), b"appimage");
    let cache_mode = std::fs::metadata(cache.path())
        .expect("cache")
        .permissions()
        .mode();
    assert_eq!(cache_mode & 0o777, 0o700);
}

#[test]
fn artifact_write_refuses_a_path_that_already_exists() {
    let cache = cache_dir();
    let path = cache.path().join("artifact.AppImage");
    std::fs::write(&path, b"planted").expect("existing file");

    let err = crate::client::write_new_private_file(&path, b"verified").expect_err("exists");

    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&path).expect("read"), b"planted");
}

#[cfg(unix)]
#[test]
fn artifact_write_does_not_follow_a_link_at_its_path() {
    let cache = cache_dir();
    let outside_dir = cache_dir();
    let outside = outside_dir.path().join("victim");
    let path = cache.path().join("artifact.AppImage");
    std::os::unix::fs::symlink(&outside, &path).expect("plant link");

    let err = crate::client::write_new_private_file(&path, b"verified").expect_err("link");

    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert!(!outside.exists(), "the write followed the planted link");
}

#[test]
fn package_managed_copy_reports_the_version_and_refuses_to_install() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_newer_release(
        &server,
        &pk,
        &sk,
        "/Oikonomia.AppImage",
        b"appimage",
        cache.path(),
    )
    .with_install_route(InstallRoute::PackageManager);
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();

    let status = machine.check(&config);

    assert_eq!(
        status,
        UpdateStatus::AvailableManually {
            version: "0.2.0".into(),
            notes: "ok".into(),
        }
    );
    let err = machine
        .install(&config, &installer)
        .expect_err("a package-managed copy must not install");
    assert_eq!(err.code(), "update_install_not_allowed");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn download_and_verify_rejects_mismatched_hash_and_leaves_no_file() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"real-bytes";
    let artifact = server_url(&server, "/Oikonomia.AppImage");
    let body = static_manifest(
        "0.2.0",
        "n",
        artifact.as_str(),
        &sign(&sk, payload),
        &sha256_hex(b"nope"),
    );
    let sig = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(status_code(200).body(payload.as_slice())),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let mut machine = UpdateMachine::new();
    machine.check(&config);
    let offer = machine.require_available().expect("offer").clone();
    let err = download_and_verify(&config, &offer).expect_err("hash");
    assert_eq!(err.code(), "update_artifact_integrity");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn raw_ed25519_hex_is_not_a_minisign_public_key() {
    const RAW_KEY_HEX: &str = "7d5b038e9ab30eef536cc559baac20e44070adedcdf548af48744029804ec671";
    let err = parse_public_key(RAW_KEY_HEX).expect_err("raw hex key");
    assert_eq!(err.code(), "update_missing_public_key");
}

/// Proves the promote lane (`assemble_manifest`) and the client (`perform_check`,
/// `download_and_verify`) agree on the wire shape: a manifest built the same way
/// `assemble_feed` builds it for a published release, served over httptest,
/// must check as `Available` and its artifact must download and verify.
#[test]
fn promoted_feed_round_trips_through_check_and_download() {
    let (pk, sk) = test_keys();
    let server = Server::run();

    let artifact_bytes = b"new-app-bytes".to_vec();
    let artifact_signature = sign(&sk, &artifact_bytes);
    let artifact_sha256_hex = sha256_hex(&artifact_bytes);
    let base_url = server_url(&server, "/").to_string();

    let manifest_body = assemble_manifest(
        "9.9.9",
        "Promoted release.",
        &base_url,
        &[FeedArtifact {
            platform: "linux-x86_64".to_owned(),
            file_name: "Oikonomia_test.app.tar.gz".to_owned(),
            signature: artifact_signature,
            sha256_hex: artifact_sha256_hex,
        }],
    )
    .expect("assemble");
    let manifest_sig = sign(&sk, manifest_body.as_bytes());

    serve_signed_manifest(&server, &manifest_body, &manifest_sig);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia_test.app.tar.gz"))
            .respond_with(status_code(200).body(artifact_bytes.clone())),
    );

    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    let outcome = perform_check(&config);
    let CheckOutcome::Available(offer) = outcome else {
        panic!("promoted feed must yield Available, got {outcome:?}");
    };
    let path = download_and_verify(&config, &offer).expect("download");
    assert!(path.exists());
    assert_eq!(leftover_files(cache.path()).len(), 1);
}

/// Returns a manifest offering `version` at `/Oikonomia.AppImage` on `server`
/// and the detached signature over it.
fn signed_manifest(server: &Server, secret_key: &SecretKey, version: &str) -> (String, String) {
    let payload = b"artifact-bytes";
    let body = static_manifest(
        version,
        "notes",
        server_url(server, "/Oikonomia.AppImage").as_str(),
        &sign(secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(secret_key, body.as_bytes());
    (body, signature)
}

fn redirect_to(location: &str) -> impl httptest::responders::Responder + use<> {
    status_code(302).append_header("Location", location.to_owned())
}

/// Returns the code of the error the check ends with.
fn check_error_code(config: &ClientConfig) -> &'static str {
    match perform_check_inner(config) {
        Ok(outcome) => panic!("the check must fail, got {outcome:?}"),
        Err(err) => err.code(),
    }
}

#[test]
fn feed_redirect_to_an_allowed_host_is_followed() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &sk, "0.2.0");
    let moved = server_url(&server, "/assets/latest.json");
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(redirect_to(moved.as_str())),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/assets/latest.json"))
            .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .respond_with(status_code(200).body(signature)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    let status = UpdateMachine::new().check(&config);

    assert_eq!(
        status,
        UpdateStatus::Available {
            version: "0.2.0".into(),
            notes: "notes".into(),
        }
    );
}

#[test]
fn feed_redirect_to_a_host_off_the_allow_list_is_refused() {
    let (pk, _sk) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(redirect_to("http://evil.example/latest.json")),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_artifact_url");
}

#[test]
fn a_chain_of_exactly_the_redirect_limit_is_followed() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &sk, "0.2.0");
    // Relative locations: each hop is resolved against the URL that sent it.
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(redirect_to("/hop/1")),
    );
    for hop in 1..MAX_REDIRECTS {
        server.expect(
            Expectation::matching(request::method_path("GET", format!("/hop/{hop}")))
                .respond_with(redirect_to(&format!("/hop/{}", hop + 1))),
        );
    }
    server.expect(
        Expectation::matching(request::method_path("GET", format!("/hop/{MAX_REDIRECTS}")))
            .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .respond_with(status_code(200).body(signature)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    let status = UpdateMachine::new().check(&config);

    assert!(
        matches!(status, UpdateStatus::Available { .. }),
        "got {status:?}"
    );
}

#[test]
fn one_redirect_past_the_limit_fails_without_another_request() {
    let (pk, _sk) = test_keys();
    let server = Server::run();
    // The first request plus one per followed redirect; the redirect that
    // answers the last of them is the one past the limit.
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .times(usize::from(MAX_REDIRECTS) + 1)
            .respond_with(redirect_to("/latest.json")),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_network");
}

#[test]
fn redirect_without_a_location_fails() {
    let (pk, _sk) = test_keys();
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(302)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_network");
}

/// Serves a signed 0.2.0 manifest whose artifact URL answers with a redirect
/// to `location`, and returns the offer a check finds there.
fn offer_with_redirected_artifact(
    server: &Server,
    secret_key: &SecretKey,
    config: &ClientConfig,
    location: &str,
) -> VerifiedOffer {
    let (body, signature) = signed_manifest(server, secret_key, "0.2.0");
    serve_signed_manifest(server, &body, &signature);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .respond_with(redirect_to(location)),
    );

    match perform_check(config) {
        CheckOutcome::Available(offer) => offer,
        outcome => panic!("expected an offer, got {outcome:?}"),
    }
}

#[test]
fn artifact_redirect_to_an_allowed_host_is_downloaded_and_verified() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let moved = server_url(&server, "/cdn/Oikonomia.AppImage");
    let offer = offer_with_redirected_artifact(&server, &sk, &config, moved.as_str());
    server.expect(
        Expectation::matching(request::method_path("GET", "/cdn/Oikonomia.AppImage"))
            .respond_with(status_code(200).body("artifact-bytes")),
    );

    let path = download_and_verify(&config, &offer).expect("download");

    assert_eq!(std::fs::read(&path).expect("read"), b"artifact-bytes");
}

#[test]
fn artifact_redirect_to_a_host_off_the_allow_list_is_refused() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let offer =
        offer_with_redirected_artifact(&server, &sk, &config, "http://evil.example/payload");

    let err = download_and_verify(&config, &offer).expect_err("redirect off the allow-list");

    assert_eq!(err.code(), "update_artifact_url");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

/// Reads from `stream` up to the blank line that ends an HTTP request head.
fn read_request_head(stream: &mut TcpStream) {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).expect("request head");
        head.push(byte[0]);
    }
}

/// Answers the first request on a fresh loopback port with `total_bytes` zero
/// bytes, written `piece_bytes` at a time with `pause` before each write.
///
/// httptest sends a body in one piece; this server exists for the tests that
/// need a body to arrive slowly or to be larger than is worth holding twice.
fn serve_zero_bytes(total_bytes: usize, piece_bytes: usize, pause: Duration) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");

    std::thread::spawn(move || {
        let (mut stream, _peer) = listener.accept().expect("accept");
        read_request_head(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {total_bytes}\r\nConnection: close\r\n\r\n"
        )
        .expect("response head");

        let piece = vec![0_u8; piece_bytes];
        let mut remaining = total_bytes;
        while remaining > 0 {
            std::thread::sleep(pause);
            let length = remaining.min(piece_bytes);
            // A client that stops at its size cap closes the connection with
            // bytes still unsent; that ends this server, it is not a failure.
            if stream.write_all(&piece[..length]).is_err() {
                return;
            }
            remaining -= length;
        }
    });

    address
}

/// Serves a signed 0.2.0 manifest on `server` whose artifact is `payload` at
/// `artifact_address`, and returns a config for an app at 0.1.0 that may
/// fetch from both.
fn config_for_artifact_at(
    server: &Server,
    keys: (&str, &SecretKey),
    artifact_address: SocketAddr,
    payload: &[u8],
    cache: &Path,
) -> ClientConfig {
    let (public_key, secret_key) = keys;
    let body = static_manifest(
        "0.2.0",
        "notes",
        &format!("http://{artifact_address}/Oikonomia.AppImage"),
        &sign(secret_key, payload),
        &sha256_hex(payload),
    );
    let signature = sign(secret_key, body.as_bytes());
    serve_signed_manifest(server, &body, &signature);

    let feed_host = server_url(server, "/").host_str().expect("host").to_owned();
    ClientConfig::for_test(
        server_url(server, "/latest.json"),
        public_key,
        "0.1.0",
        "linux-x86_64",
        cache.to_path_buf(),
        HostPolicy::test_http_hosts([feed_host, artifact_address.ip().to_string()]),
        Duration::from_millis(500),
    )
    .expect("config")
}

fn available_offer(config: &ClientConfig) -> VerifiedOffer {
    match perform_check(config) {
        CheckOutcome::Available(offer) => offer,
        outcome => panic!("expected an offer, got {outcome:?}"),
    }
}

#[test]
fn artifact_download_may_outlast_the_feed_deadline_while_bytes_keep_arriving() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    // Four writes 300 ms apart: 1.2 s in all, against a 500 ms feed deadline.
    let payload = [0_u8; 4];
    let artifact_address = serve_zero_bytes(payload.len(), 1, Duration::from_millis(300));
    let config = config_for_artifact_at(
        &server,
        (&pk, &sk),
        artifact_address,
        &payload,
        cache.path(),
    );
    let offer = available_offer(&config);

    let path = download_and_verify(&config, &offer).expect("slow download");

    assert_eq!(std::fs::read(&path).expect("read"), payload);
}

#[test]
fn artifact_fields_outside_the_platform_table_are_not_an_offer() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let payload = b"artifact-bytes";
    // No entry for this platform; the artifact sits at the top level, a
    // shape `assemble_manifest` never writes.
    let body = serde_json::json!({
        "version": "0.2.0",
        "notes": "notes",
        "platforms": {},
        "url": server_url(&server, "/Oikonomia.AppImage").as_str(),
        "signature": sign(&sk, payload),
        "sha256": sha256_hex(payload),
    })
    .to_string();
    let signature = sign(&sk, body.as_bytes());
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_missing_platform");
}

/// Serves a signed manifest for `version` that lists an artifact for
/// `darwin-aarch64` only, and returns a config for a `linux-x86_64` app at
/// 0.1.0.
fn serve_release_without_this_platform(
    server: &Server,
    public_key: &str,
    secret_key: &SecretKey,
    version: &str,
    cache: &Path,
) -> ClientConfig {
    let payload = b"artifact-bytes";
    let body = assemble_manifest(
        version,
        "notes",
        server_url(server, "/").as_str(),
        &[FeedArtifact {
            platform: "darwin-aarch64".to_owned(),
            file_name: "Oikonomia.app.tar.gz".to_owned(),
            signature: sign(secret_key, payload),
            sha256_hex: sha256_hex(payload),
        }],
    )
    .expect("assemble");
    let signature = sign(secret_key, body.as_bytes());
    serve_signed_manifest(server, &body, &signature);

    config(
        server,
        "/latest.json",
        public_key,
        "0.1.0",
        cache,
        Duration::from_secs(2),
    )
}

#[test]
fn current_version_is_up_to_date_even_when_the_feed_omits_this_platform() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_release_without_this_platform(&server, &pk, &sk, "0.1.0", cache.path());

    assert_eq!(UpdateMachine::new().check(&config), UpdateStatus::UpToDate);
}

#[test]
fn newer_version_without_this_platform_fails_as_a_missing_platform() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let config = serve_release_without_this_platform(&server, &pk, &sk, "0.2.0", cache.path());

    assert_eq!(check_error_code(&config), "update_missing_platform");
}

#[test]
fn feed_and_signature_requests_each_name_this_copy_once() {
    const IDENTITY_QUERY: &str = "version=0.1.0&os=linux&arch=x86_64";

    let (pk, sk) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &sk, "0.2.0");
    server.expect(
        Expectation::matching(httptest::all_of![
            request::method_path("GET", "/latest.json"),
            request::query(IDENTITY_QUERY),
        ])
        .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(httptest::all_of![
            request::method_path("GET", "/latest.json.sig"),
            request::query(IDENTITY_QUERY),
        ])
        .respond_with(status_code(200).body(signature)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    let status = UpdateMachine::new().check(&config);

    assert!(
        matches!(status, UpdateStatus::Available { .. }),
        "got {status:?}"
    );
}

/// Keeps every warning the crate logs while the tests run.
///
/// `log` takes one logger per process, so all tests share this one and each
/// looks only for messages that name a path of its own.
struct CapturedWarnings {
    messages: Mutex<Vec<String>>,
}

impl CapturedWarnings {
    fn mentioning(&self, path: &Path) -> Vec<String> {
        let needle = path.display().to_string();
        let messages = self.messages.lock().expect("captured warnings");
        messages
            .iter()
            .filter(|message| message.contains(&needle))
            .cloned()
            .collect()
    }
}

impl log::Log for CapturedWarnings {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let mut messages = self.messages.lock().expect("captured warnings");
            messages.push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

fn captured_warnings() -> &'static CapturedWarnings {
    static WARNINGS: CapturedWarnings = CapturedWarnings {
        messages: Mutex::new(Vec::new()),
    };
    static INSTALL: std::sync::Once = std::sync::Once::new();

    INSTALL.call_once(|| {
        log::set_logger(&WARNINGS).expect("this test binary installs no other logger");
        log::set_max_level(log::LevelFilter::Warn);
    });
    &WARNINGS
}

#[test]
fn delete_artifact_removes_the_file() {
    let cache = cache_dir();
    let artifact = cache.path().join("Oikonomia.AppImage");
    std::fs::write(&artifact, b"verified").expect("artifact");

    delete_artifact(&artifact);

    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn delete_artifact_says_nothing_about_a_file_that_is_already_gone() {
    let warnings = captured_warnings();
    let cache = cache_dir();
    let missing = cache.path().join("never-written.AppImage");

    delete_artifact(&missing);

    assert_eq!(warnings.mentioning(&missing), Vec::<String>::new());
}

#[test]
fn delete_artifact_logs_a_removal_that_fails() {
    let warnings = captured_warnings();
    let cache = cache_dir();
    // `remove_file` refuses a directory, with an error other than `NotFound`.
    let directory = cache.path().join("not-a-file");
    std::fs::create_dir(&directory).expect("directory");

    delete_artifact(&directory);

    assert_eq!(warnings.mentioning(&directory).len(), 1);
}

/// Returns a manifest offering 0.2.0, padded with trailing whitespace (which
/// JSON allows) to exactly `total_bytes`, and the signature over it.
fn signed_manifest_of_size(
    server: &Server,
    secret_key: &SecretKey,
    total_bytes: usize,
) -> (String, String) {
    let (mut body, _signature) = signed_manifest(server, secret_key, "0.2.0");
    let padding = total_bytes
        .checked_sub(body.len())
        .expect("the manifest is smaller than the requested size");
    body.push_str(&" ".repeat(padding));

    let signature = sign(secret_key, body.as_bytes());
    (body, signature)
}

#[test]
fn manifest_of_exactly_the_size_cap_is_accepted() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest_of_size(&server, &sk, MAX_MANIFEST_BYTES);
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    let status = UpdateMachine::new().check(&config);

    assert!(
        matches!(status, UpdateStatus::Available { .. }),
        "got {status:?}"
    );
}

#[test]
fn manifest_one_byte_over_the_size_cap_is_refused() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    // Validly signed, so only its size stands between it and an offer.
    let (body, _signature) = signed_manifest_of_size(&server, &sk, MAX_MANIFEST_BYTES + 1);
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(200).body(body)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_network");
}

#[test]
fn manifest_signature_over_the_size_cap_is_refused() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let (body, mut signature) = signed_manifest(&server, &sk, "0.2.0");
    // `verify_minisign` trims its input, so read in full the padded signature
    // would verify and the check would end in an offer.
    let padding = MAX_SIGNATURE_BYTES + 1 - signature.len();
    signature.push_str(&"\n".repeat(padding));
    serve_signed_manifest(&server, &body, &signature);
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_network");
}

#[test]
fn artifact_over_the_size_cap_is_refused_and_leaves_no_file() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let cache = cache_dir();
    let artifact_address = serve_zero_bytes(MAX_ARTIFACT_BYTES + 1, 1 << 20, Duration::ZERO);
    // The manifest describes other bytes: read in full, the download would
    // end as an integrity error, not a network one.
    let config = config_for_artifact_at(
        &server,
        (&pk, &sk),
        artifact_address,
        b"artifact-bytes",
        cache.path(),
    );
    let offer = available_offer(&config);

    let err = download_and_verify(&config, &offer).expect_err("over the cap");

    assert_eq!(err.code(), "update_network");
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn older_remote_version_is_up_to_date_and_never_downloaded() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let (body, signature) = signed_manifest(&server, &sk, "0.0.9");
    serve_signed_manifest(&server, &body, &signature);
    server.expect(
        Expectation::matching(request::method_path("GET", "/Oikonomia.AppImage"))
            .times(0)
            .respond_with(status_code(500)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );
    let (installer, calls) = spy(false);
    let mut machine = UpdateMachine::new();

    assert_eq!(machine.check(&config), UpdateStatus::UpToDate);

    let err = machine
        .install(&config, &installer)
        .expect_err("an older version must not be installable");
    assert_eq!(err.code(), "update_install_not_allowed");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(leftover_files(cache.path()), Vec::<PathBuf>::new());
}

#[test]
fn http_204_on_the_manifest_signature_is_a_signature_failure() {
    let (pk, sk) = test_keys();
    let server = Server::run();
    let (body, _signature) = signed_manifest(&server, &sk, "0.2.0");
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json"))
            .respond_with(status_code(200).body(body)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/latest.json.sig"))
            .respond_with(status_code(204)),
    );
    let cache = cache_dir();
    let config = config(
        &server,
        "/latest.json",
        &pk,
        "0.1.0",
        cache.path(),
        Duration::from_secs(2),
    );

    assert_eq!(check_error_code(&config), "update_manifest_signature");
}
